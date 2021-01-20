use std::collections::HashMap;
use std::fs::{self, File};
use std::io::prelude::*;
use std::path::{PathBuf, Path};
use std::str::Chars;

use futures::stream::{StreamExt, iter as siter};
use reqwest::Client;
use select::{
    document::Document,
    node::Node,
    predicate::{Class, Name, Predicate, Text},
};
use serde::{Deserialize, Serialize};

const BRANCHES_DIR: &'static str = "dest/branches";

fn prepare_dest_dir() {
    let _ = fs::create_dir_all(BRANCHES_DIR);
}

#[derive(Debug)]
enum Error {
    FetchBankError(reqwest::Error),
    FechBranchError(reqwest::Error),
    LoadBanksFileFailed(serde_json::Error),
    SaveBankFileFailed(std::io::Error),
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
struct Bank {
    name: String,
    phonetic: String,
    code: BankCode,
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

    fn to_hashmap(&self) -> HashMap<BankCode, Self> {
        let mut data = HashMap::new();
        data.insert(self.code.clone(), self.clone());
        data
    }

    fn filepath(&self) -> PathBuf {
        let mut path = PathBuf::new();
        path.push(format!("{}.json", &self.code.0));
        path
    }

    fn append_branch(&mut self, branch: Branch) {
        self.branches.push(branch)
    }

    async fn save_as_file(&self) -> Result<(), Error>{
        let filepath = self.filepath();
        let hashmap = self.to_hashmap();
        let mut file = File::create(&filepath).map_err(Error::SaveBankFileFailed)?;
        let data = serde_json::to_string(&hashmap).unwrap();
        let mut stream = siter(data.as_bytes().chunks(100));
        while let Some(content) = stream.next().await {
            file.write(content);
        }
        Ok(())
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

    async fn fetch_all_branches(&mut self, client: Client, search_keys: Chars<'static>) -> Result<Self, Error>{
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
    let data = to_hashmap(&banks);
    let _ = file.write_all(serde_json::to_string(&data).unwrap().as_bytes());
}

fn load_banks() -> Result<HashMap<BankCode, Bank>, Error> {
    let dest_path = Path::new(BANKS_JSON);
    let file = File::open(dest_path).unwrap();
    serde_json::from_reader(&file).map_err(Error::LoadBanksFileFailed)
}


#[derive(Debug, Deserialize, Serialize, Eq, PartialEq, Hash, Clone)]
struct BankCode(String);

fn to_hashmap(banks: &Vec<Bank>) -> HashMap<BankCode, Bank> {
    let mut data = HashMap::new();
    for bank in banks.iter() {
        data.extend(bank.to_hashmap());
    }
    data
}

async fn iterate_banks(client: &Client, banks: &mut Vec<Bank>) -> Result<(), Error>{
    for bank in banks.iter_mut() {
        let client = client.clone();
        let search_keys = all_search_keys();
        let bank = bank.fetch_all_branches(client, search_keys).await?;
        bank.save_as_file().await?;
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let client = Client::new();
    let search_keys = all_search_keys();
    let banks = fetch_all_banks(client.clone(), search_keys).await;
    save_banks(&banks);
    let data = load_banks().unwrap();
    let mut bank = data.get(&BankCode("2740".to_owned())).unwrap().clone();
    let search_keys = "う".chars();
    let branches = bank.fetch_all_branches(client.clone(), search_keys).await;
    println!("{:?}", &branches);
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

        let result = to_hashmap(&banks);
        assert_eq!(result[&bank1.code], bank1);
        assert_eq!(result[&bank2.code], bank2);
    }
}