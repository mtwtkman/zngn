use std::fs::{self, File};
use std::io::prelude::*;
use std::path::Path;
use std::str::Chars;

use reqwest::Client;
use select::{
    document::Document,
    predicate::{Class, Name, Predicate},
};
use serde::{Deserialize, Serialize};

fn prepare_dest_dir() {
    let _ = fs::create_dir("dest");
}

#[derive(Debug)]
enum Error {
    FetchBankError(reqwest::Error),
    FechBranchError(reqwest::Error),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Bank {
    name: String,
    phonetic: String,
    code: String,
    search_param: String,
    branches: Vec<Branch>,
}

impl Bank {
    fn new(name: String, phonetic: String, code: String, search_param: String) -> Self {
        Self {
            name,
            phonetic,
            code,
            search_param,
            branches: Vec::new(),
        }
    }
}

async fn fetch_branches(bank: Bank, client: Client, search_key: char) -> Result<Vec<Branch>, Error> {
    let html = client
        .post("https://zengin.ajtw.net/shitenmeisai.php")
        .form(&[("sm", search_key.to_string()), ("pz", bank.search_param.clone())])
        .send()
        .await
        .map_err(Error::FechBranchError)?
        .text()
        .await
        .unwrap();
    Ok(parse_branches(html))
}

async fn fetch_all_branches(bank: Bank, client: Client, search_keys: Chars<'static>) -> Vec<Branch> {
    let future = futures::future::join_all(
        search_keys
            .map(|search_key| {
                let client = client.clone();
                let bank = bank.clone();
                tokio::spawn( async move {
                    fetch_branches(bank, client, search_key).await
                })
            })
    );
    future
        .await
        .into_iter()
        .filter(|task_result| task_result.is_ok())
        .map(|task_result| task_result.unwrap().unwrap())
        .flatten()
        .collect::<Vec<Branch>>()

}

fn parse_branches(html: String) -> Vec<Branch> {
    let document = Document::from(html.as_str());
    document
        .find(Name("tbody").descendant(Name("tr")))
        .map(|node| {
            let mut datarows = node.children();
            let name = datarows.next().unwrap().text();
            let phonetic = datarows.next().unwrap().text();
            let code = datarows.next().unwrap().text();
            Branch {
                name,
                phonetic,
                code
            }
        })
        .collect::<Vec<Branch>>()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Branch {
    name: String,
    phonetic: String,
    code: String,
}

async fn fetch_bank_list(client: Client, search_key: char) -> Result<Vec<Bank>, Error> {
    let html = client
        .post("https://zengin.ajtw.net/ginkou.php")
        .form(&[("gm", &search_key.to_string())])
        .send()
        .await
        .map_err(Error::FetchBankError)?
        .text()
        .await
        .unwrap();
    Ok(parse_bank_list(html))
}

fn parse_bank_list(html: String) -> Vec<Bank> {
    let document = Document::from(html.as_str());
    document
        .find(Class("j0").descendant(Name("tbody").descendant(Name("tr"))))
        .map(|node| {
            let mut datarows = node.children();
            let name = datarows.next().unwrap().text();
            let phonetic = datarows.next().unwrap().text();
            let code = datarows.next().unwrap().text();
            let search_param = datarows
                .next()
                .unwrap()
                .find(Name("button"))
                .next()
                .unwrap()
                .attr("value")
                .unwrap()
                .to_owned();
            Bank::new(
                name,
                phonetic,
                code,
                search_param,
            )
        })
        .collect::<Vec<Bank>>()
}

fn all_search_keys() -> Chars<'static> {
    "
    あいうえお
    かきくけこ
    さしすせそ
    たちつてと
    なにぬねの
    はひふへほ
    まみむめも
    やゆよ
    らりるれろ
    わ
    ".chars()
}

async fn fetch_all_bank_list(client: Client, search_keys: Chars<'static>) -> Vec<Bank> {
    let future = futures::future::join_all(
        search_keys
            .map(|search_key| {
                let client = client.clone();
                tokio::spawn(async move {
                    fetch_bank_list(client, search_key).await
                })
            })
    );
    future
        .await
        .into_iter()
        .filter(|task_result| task_result.is_ok())  // FIXME: handle JoinError
        .map(|task_result| task_result.unwrap().unwrap())  // FIXME: save failed requests
        .flatten()
        .collect::<Vec<Bank>>()
}

const BANK_LIST_JSON: &'static str = "dest/bank_list.json";

fn save_bank_list(bank_list: &Vec<Bank>) {
    let dest_path = Path::new(BANK_LIST_JSON);
    let mut file = File::create(dest_path).unwrap();
    let _ = file.write_all(serde_json::to_string(bank_list).unwrap().as_bytes());
}

fn load_bank_list() -> Vec<Bank> {
    let dest_path = Path::new(BANK_LIST_JSON);
    let file = File::open(dest_path).unwrap();
    serde_json::from_reader(&file).unwrap()
}

#[tokio::main]
async fn main() {
    // prepare_dest_dir();
    // let client = Client::new();
    // let search_keys = "あい".chars();
    // let result = fetch_all_bank_list(client, search_keys).await;
    // save_bank_list(&result);
    let bank = Bank::new(
        "アイオー信用金庫".to_owned(),
        "ｱｲｵ-ｼﾝｷﾝ".to_owned(),
        "1206".to_owned(),
        "1206xあ9".to_owned(),
    );
    let client = Client::new();
    let search_keys = "あいうえお".chars();
    let result = fetch_all_branches(bank, client, search_keys).await;
    println!("{:?}", &result);
}
