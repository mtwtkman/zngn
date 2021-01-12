use std::collections::HashMap;
use std::fs::{self, File};
use std::io::prelude::*;
use std::path::Path;
use std::str::Chars;

use reqwest::Client;
use select::{
    document::Document,
    node::Node,
    predicate::{Class, Name, Predicate, Text},
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

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
struct Bank {
    name: String,
    phonetic: String,
    code: BankCode,
    #[serde(skip)]
    search_param: String,
    branches: Vec<Branch>,
}

impl Bank {
    fn new(name: String, phonetic: String, code: String, search_param: String) -> Self {
        Self {
            name,
            phonetic,
            code: BankCode(code),
            search_param,
            branches: Vec::new(),
        }
    }

    fn append_branch(&mut self, branch: Branch) {
        self.branches.push(branch)
    }

    async fn fetch_branches(&self, client: Client, search_key: char) -> Result<Vec<Branch>, Error> {
        let html = client
            .post("https://zengin.ajtw.net/shitenmeisai.php")
            .form(&[("sm", search_key.to_string()), ("pz", self.search_param.clone())])
            .send()
            .await
            .map_err(Error::FechBranchError)?
            .text()
            .await
            .unwrap();
        Ok(parse_branches(html))
    }

    async fn fetch_all_branches(mut self, client: Client, search_keys: Chars<'static>) -> Result<Self, Error>{
        let future = futures::future::join_all(
            search_keys
                .clone()
                .map(|search_key| {
                    let client = client.clone();
                    let bank = self.clone();
                    tokio::spawn( async move {
                        bank.fetch_branches(client, search_key).await
                    })
                })
        );
        self.branches = future
            .await
            .into_iter()
            .filter(|task_result| task_result.is_ok())
            .map(|task_result| task_result.unwrap().unwrap())
            .flatten()
            .collect::<Vec<Branch>>();
        Ok(self.clone())
    }
}

fn filter_blank(node: &Node) -> bool {
    let text = node.find(Text).next();
    if text.is_none() {
        return false;
    }
    text.unwrap().text() != "該当するデータはありません"
}

fn parse_branches(html: String) -> Vec<Branch> {
    let document = Document::from(html.as_str());
    document
        .find(Name("tbody").descendant(Name("tr")))
        .filter(filter_blank)
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

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
struct Branch {
    name: String,
    phonetic: String,
    code: String,
}

impl Branch {
    fn new(name: String, phonetic: String, code: String) -> Self {
        Self {
            name,
            phonetic,
            code,
        }
    }
}

async fn fetch_banks(client: Client, search_key: char) -> Result<Vec<Bank>, Error> {
    let html = client
        .post("https://zengin.ajtw.net/ginkou.php")
        .form(&[("gm", &search_key.to_string())])
        .send()
        .await
        .map_err(Error::FetchBankError)?
        .text()
        .await
        .unwrap();
    Ok(parse_banks(html))
}

fn parse_banks(html: String) -> Vec<Bank> {
    let document = Document::from(html.as_str());
    document
        .find(Class("j0").descendant(Name("tbody").descendant(Name("tr"))))
        .filter(filter_blank)
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
    "あいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほまみむめもやゆよらりるれろわ".chars()
}

async fn fetch_all_banks(client: Client, search_keys: Chars<'static>) -> Vec<Bank> {
    let future = futures::future::join_all(
        search_keys
            .clone()
            .map(|search_key| {
                let client = client.clone();
                tokio::spawn(async move {
                    fetch_banks(client, search_key).await
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

const BANKS_JSON: &'static str = "dest/banks.json";

fn save_banks(banks: &Vec<Bank>) {
    let dest_path = Path::new(BANKS_JSON);
    let mut file = File::create(dest_path).unwrap();
    let _ = file.write_all(serde_json::to_string(banks).unwrap().as_bytes());
}

fn load_banks() -> Vec<Bank> {
    let dest_path = Path::new(BANKS_JSON);
    let file = File::open(dest_path).unwrap();
    serde_json::from_reader(&file).unwrap()
}


#[derive(Debug, Serialize, Eq, PartialEq, Hash, Clone, Deserialize)]
struct BankCode(String);

fn to_hashmap(banks: Vec<Bank>) -> HashMap<BankCode, Bank> {
    let mut data = HashMap::new();
    for bank in banks.into_iter() {
        data.insert(bank.code.clone(), bank);
    }
    data
}

#[tokio::main]
async fn main() {
    let client = Client::new();
    let search_kesy = all_search_keys();
    let banks = fetch_all_banks(client, search_kesy).await;
    save_banks(&banks);
    println!("DONE");
}

#[cfg(test)]
mod tests {
    #[test]
    fn to_hashmap_test() {
        use crate::{Bank, Branch, to_hashmap};

        let mut bank1 = Bank::new("ねこ銀行".to_owned(), "ﾈｺ".to_owned(), "0222".to_owned(), "0x222".to_owned());
        let branch1_1 = Branch::new("みけ支店".to_owned(), "ﾐｹ".to_owned(), "0123".to_owned());
        let branch1_2 = Branch::new("とら支店".to_owned(), "ﾄﾗ".to_owned(), "0789".to_owned());
        bank1.append_branch(branch1_1);
        bank1.append_branch(branch1_2);

        let mut bank2 = Bank::new("いぬ銀行".to_owned(), "ｲﾇ".to_owned(), "0111".to_owned(), "0x111".to_owned());
        let branch2_1 = Branch::new("しば支店".to_owned(), "ｼﾊﾞ".to_owned(), "0345".to_owned());
        let branch2_2 = Branch::new("かい支店".to_owned(), "ｶｲ".to_owned(), "0456".to_owned());
        bank2.append_branch(branch2_1);
        bank2.append_branch(branch2_2);

        let banks = vec![bank1.clone(), bank2.clone()];

        let result = to_hashmap(banks);
        assert_eq!(result[&bank1.code], bank1);
        assert_eq!(result[&bank2.code], bank2);
    }
}