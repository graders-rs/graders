use failure::Error;
use graders_utils::amqputils::AMQPConfiguration;
use serde_yaml;
use std::fs::File;
use std::io::Read;
use tester::TesterConfiguration;

#[derive(Deserialize)]
pub struct Configuration {
    pub amqp: AMQPConfiguration,
    pub tester: TesterConfiguration,
}

pub fn load_configuration(file: &str) -> Result<Configuration, Error> {
    let mut f = File::open(file)?;
    let mut content = Vec::new();
    f.read_to_end(&mut content)?;
    Ok(serde_yaml::from_slice(&content)?)
}
