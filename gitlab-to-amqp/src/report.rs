use amqp_utils::AmqpResponse;
use failure::Error;
use gitlab::api::{self, State};
use hyper::Request;

use crate::config::Configuration;
use crate::gitlab;

#[derive(Deserialize)]
pub struct Report {
    grade: usize,
    #[serde(rename = "max-grade")]
    max_grade: usize,
    explanation: Option<String>,
    groups: Option<Vec<Group>>,
}

#[derive(Deserialize)]
pub struct Group {
    grade: usize,
    #[serde(rename = "max-grade")]
    max_grade: usize,
    description: Option<String>,
    tests: Vec<Test>,
}

#[derive(Deserialize)]
pub struct Test {
    coefficient: usize,
    description: String,
    success: bool,
    signal: Option<u32>,
}

fn signal_to_explanation(signal: u32) -> String {
    match signal as i32 {
        libc::SIGILL => String::from("illegal instruction"),
        libc::SIGABRT => String::from("abort, possibly because of a failed assertion"),
        libc::SIGFPE => String::from("arithmetic exception"),
        libc::SIGKILL => String::from(
            "program killed, possibly because of an infinite loop or memory exhaustion",
        ),
        libc::SIGBUS => String::from("bus error"),
        libc::SIGSEGV => String::from("segmentation fault"),
        s => format!("crash (signal {})", s),
    }
}

fn yaml_to_markdown(lab: &str, yaml: &str) -> Result<(String, usize, usize), Error> {
    let report: Report = serde_yaml::from_str(yaml)?;
    if let Some(explanation) = report.explanation {
        warn!("problem during handling of {}: {}", lab, explanation);
        return Ok((
            format!(
                r#"## Error

There has been an error during the test for {}:

```
{}
```"#,
                lab, explanation
            ),
            report.grade,
            report.max_grade,
        ));
    }
    let groups = report
        .groups
        .unwrap_or_else(Vec::new)
        .iter()
        .filter(|group| group.grade != group.max_grade)
        .map(|group| {
            let tests = if group.grade != 0 {
                let mut explanation = "Failing tests:\n\n".to_owned();
                explanation.push_str(
                    &group
                        .tests
                        .iter()
                        .filter(|test| !test.success)
                        .map(|test| {
                            format!(
                                "- {}{}{}",
                                &test.description,
                                if test.coefficient != 1 {
                                    format!(" (coefficient {})", test.coefficient)
                                } else {
                                    "".to_owned()
                                },
                                test.signal
                                    .map(|s| format!(" [{}]", signal_to_explanation(s)))
                                    .unwrap_or_else(|| "".to_owned()),
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
                explanation
            } else {
                String::new()
            };
            format!(
                "### {} ({})\n\n{}\n",
                group
                    .description
                    .clone()
                    .unwrap_or_else(|| "*Test group*".to_owned()),
                pass_fail(group.grade, group.max_grade),
                tests
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let diagnostic = format!(
        "## Failed tests report for {} ({})\n\n{}",
        lab,
        pass_fail(report.grade, report.max_grade),
        groups
    );
    Ok((diagnostic, report.grade, report.max_grade))
}

pub fn response_to_post(
    config: &Configuration,
    response: &AmqpResponse,
) -> Result<Vec<Request<String>>, Error> {
    let (report, grade, max_grade) = yaml_to_markdown(&response.lab, &response.yaml_result)?;
    let (hook, zip) = gitlab::from_opaque(&response.opaque)?;
    match gitlab::remove_zip_file(config, &zip) {
        Ok(_) => trace!("removed zip file {}", zip),
        Err(e) => warn!("could not remove zip file {}: {}", zip, e),
    }
    let state = if grade == max_grade {
        State::Success
    } else {
        State::Failed
    };
    let status = api::post_status(
        &config.gitlab,
        &hook,
        &state,
        hook.branch_name(),
        &response.lab,
        Some(&format!("grade: {}/{}", grade, max_grade)),
    );
    Ok(if state == State::Success {
        info!(
            "tests for {} are a success, generating status only",
            &response.job_name
        );
        vec![status]
    } else {
        info!(
            "tests for {} are a failure ({}/{}), generating status and comment",
            &response.job_name, grade, max_grade
        );
        let comment = api::post_comment(&config.gitlab, &hook, &report);
        vec![status, comment]
    })
}

fn pass_fail(grade: usize, max_grade: usize) -> String {
    if grade > max_grade {
        format!("{} passing out of {} [!]", grade, max_grade)
    } else if grade == max_grade {
        format!("all {} passing", max_grade)
    } else if grade == 0 {
        format!("all {} failing", max_grade)
    } else {
        format!("{} failing out of {}", max_grade - grade, max_grade)
    }
}
