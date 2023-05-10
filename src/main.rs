use clap::{arg, App, Arg};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::{json, to_string_pretty, Result};

use std::fs;
use std::fs::File;
use std::io::Read;
use std::process::{Command, Output};
use std::str;

use base64;

static NODE: &str = "https://rpc.osmotest5.osmosis.zone:443";
static TESTNET: &str = "osmo-test-5";
static WALLET: &str = "wallet";
static CONTRACTS: &str = "config/rover-osmosis5-contracts.json";

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Attribute {
    key: String,
    value: String,
}

#[derive(Deserialize, Debug, Clone)]
struct Event {
    attributes: Vec<Attribute>,
    #[serde(rename = "type")]
    event_type: String,
}

#[derive(Deserialize, Debug)]
struct Data {
    code: i32,
    codespace: String,
    data: String,
    events: Vec<Event>,
    tx: Tx,
    logs: Vec<Log>,
}

#[derive(Debug, Deserialize, Clone)]
struct Log {
    msg_index: u32,
    log: String,
    events: Vec<Event>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Message {
    #[serde(rename = "@type")]
    type_: String,
    sender: String,
    contract: String,
    msg: serde_json::Value,
    funds: Vec<Fund>,
}
#[derive(Serialize, Deserialize, Debug)]
struct Fund {
    denom: String,
    amount: String,
}
#[derive(Serialize, Deserialize, Debug)]
struct Tx {
    #[serde(rename = "@type")]
    type_: String,
    body: Body,
}

#[derive(Serialize, Deserialize, Debug)]
struct Body {
    messages: Vec<Message>,
    memo: String,
    timeout_height: String,
    extension_options: Vec<serde_json::Value>,
    non_critical_extension_options: Vec<serde_json::Value>,
}

fn main() {
    let matches = App::new("Osmosis CLI wrapper")
        .arg(
            arg!(--cmd <command>)
                .required(true)
                .takes_value(true)
                .help("Command to execute: execute|query"),
        )
        .arg(
            arg!(--contract <contract_name>)
                .required(false)
                .takes_value(true)
                .help("Name of the contract to execute or query"),
        )
        .arg(
            arg!(--json <file>)
                .required(false)
                .takes_value(true)
                .help("Json file that contains the message"),
        )
        .arg(
            arg!(--amount <number_denom>)
                .required(false)
                .takes_value(true)
                .help("amount to send with the tx"),
        )
        .arg(
            arg!(--tx <hash>)
                .required(false)
                .takes_value(true)
                .help("tx hash"),
        )
        .get_matches();

    let cmd = matches.value_of("cmd").unwrap();
    let contract_name = matches.value_of("contract").unwrap_or("");
    let json_path = matches.value_of("json").unwrap_or("");
    let tx_hash = matches.value_of("tx").unwrap_or("");
    let amount = matches
        .value_of("amount")
        .map_or("".to_owned(), |a| "--amount=".to_owned() + a);

    match cmd {
        "execute" => execute_tx(
            get_contract_address(contract_name),
            get_json(json_path),
            amount,
        ),
        "query" => query_contract(get_contract_address(contract_name), get_json(json_path)),
        "get_tx_events" => {
            if tx_hash.is_empty() {
                println!("Need a tx hash to get events");
            } else {
                get_tx_data(tx_hash);
            }
        }

        _ => println!("Cmd should be either query or execute"),
    }
}

fn get_json(json_path: &str) -> String {
    fs::read_to_string(json_path).expect("Failed to read JSON file")
}

fn execute_tx(contract_address: String, json_str: String, amount: String) {
    let mut cmd = Command::new("osmosisd");

    cmd.arg("tx")
        .arg("wasm")
        .arg("execute")
        .arg(contract_address)
        .arg(&json_str)
        .arg("--gas-prices=0.025uosmo")
        .arg("--gas=auto")
        .arg("--gas-adjustment=1.3")
        .arg("-y")
        .arg("--keyring-backend=test")
        .arg("--output=json")
        .arg(format!("--from={}", WALLET))
        .arg(format!("--node={}", NODE))
        .arg(format!("--chain-id={}", TESTNET));

    if !amount.is_empty() {
        cmd.arg(amount);
    }

    let output = cmd.output().expect("Failed to execute command");

    print_result(output);
}

fn query_contract(contract_name: String, query_json: String) {
    let output = Command::new("osmosisd")
        .arg("query")
        .arg("wasm")
        .arg("contract-state")
        .arg("smart")
        .arg(contract_name)
        .arg(&query_json)
        .arg("--output=json")
        .arg(format!("--node={}", NODE))
        .output()
        .expect("Failed to execute command");

    print_result(output);
}

fn get_tx_data(tx_hash: &str) {
    let output = Command::new("osmosisd")
        .arg("query")
        .arg("tx")
        .arg(tx_hash)
        .arg("--output=json")
        .arg(format!("--node={}", NODE))
        .output()
        .expect("Failed to execute command");

    if output.status.success() {
        let stdout = str::from_utf8(&output.stdout).unwrap();

        println!("{}", stdout);

        let mut parsed_data: Data = serde_json::from_str(stdout).unwrap_or_else(|error| {
            panic!("Failed to parse JSON: {}", error);
        });

        let sender = parsed_data.tx.body.messages[0].sender.clone();
        let messages = parsed_data.tx.body.messages;
        let events_short: String = summarize_events(parsed_data.events, true);
        let logs_short: String = summarize_events(parsed_data.logs[0].events.clone(), false);

        println!("--> Sender <--");
        println!("{}", sender);

        println!("--> Messages <--");
        for message in messages {
            let json_str = serde_json::to_string_pretty(&message).unwrap();
            println!("Message:\n{}", json_str);
        }

        println!("--> Events <--");
        println!("{}", events_short);

        println!("--> Logs <--");
        println!("{}", logs_short);
    } else {
        // Handle command execution failure
        let stderr = str::from_utf8(&output.stderr).unwrap();
        eprintln!("Command execution failed: {}", stderr);
    }
}

fn summarize_events(mut events: Vec<Event>, encoding: bool) -> String {
    let mut events_short: String = "".to_string();

    events.iter_mut().for_each(|event| {
        if event.event_type != "tx" {
            events_short += format!("--> {}( ", event.event_type).as_str();
            for attribute in &mut event.attributes {
                let mut value = &attribute.value;
                let key = &attribute.key;

                if encoding {
                    let decoded_value = decode(&attribute.value);
                    let decoded_key = decode(&attribute.key);

                    let decoded_value_s = String::from_utf8_lossy(&decoded_value).into_owned();
                    let decoded_key_s = String::from_utf8_lossy(&decoded_key).into_owned();

                    let contract_name =
                        get_contract_name(decoded_value_s.as_str()).unwrap_or("".to_owned());
                    if !contract_name.is_empty() {
                        let formated = format!("{} ({})", decoded_value_s, contract_name);
                        events_short += format!("{}: {}, ", decoded_key_s, formated).as_str();
                    } else {
                        events_short +=
                            format!("{}: {}, ", decoded_key_s, decoded_value_s).as_str();
                    }
                } else {
                    let contract_name = get_contract_name(value).unwrap_or("".to_owned());
                    if !contract_name.is_empty() {
                        let formated = format!("{} ({})", value, contract_name);
                        events_short += format!("{}: {}, ", key, formated).as_str();
                    } else {
                        events_short += format!("{}: {}, ", key, value).as_str();
                    }
                }
            }
            events_short.truncate(events_short.len() - 2);
            events_short += " )\n";
        }
    });
    events_short.clone()
}

fn decode(encoded: &str) -> Vec<u8> {
    match base64::decode(encoded) {
        Ok(decoded) => decoded,
        Err(_) => encoded.as_bytes().to_vec(),
    }
}

fn print_result(output: Output) {
    let stdout_str = String::from_utf8(output.stdout).unwrap();
    let stderr_str = String::from_utf8(output.stderr).unwrap();
    if let Ok(json) = serde_json::from_str::<Value>(&stdout_str) {
        println!("{}", serde_json::to_string_pretty(&json).unwrap());
    } else if !stderr_str.is_empty() {
        println!("stderr:\n{}", stderr_str);
    } else {
        println!("stdout:\n{}", stdout_str);
    }
}

fn get_contract_address(contract_name: &str) -> String {
    let mut file = File::open(CONTRACTS).expect("Unable to open file");
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .expect("Unable to read file");

    let json: Value = serde_json::from_str(&contents).expect("Unable to parse JSON");
    json[contract_name]
        .as_str()
        .map(|s| s.to_owned())
        .expect("Invalid contract name")
}

fn get_contract_name(contract_address: &str) -> Option<String> {
    let mut file = File::open(CONTRACTS).expect("Unable to open file");
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .expect("Unable to read file");

    let json: Value = serde_json::from_str(&contents).expect("Unable to parse JSON");

    if let Some(map) = json.as_object() {
        for (key, value) in map.iter() {
            if let Some(address) = value.as_str() {
                if address == contract_address {
                    return Some(key.to_owned());
                }
            }
        }
    }

    None
}
