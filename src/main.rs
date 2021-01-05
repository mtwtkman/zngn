use std::str::Chars;

use reqwest::Client;
use select::{
    document::Document,
    predicate::{Class, Name, Predicate},
};

#[derive(Debug)]
enum Error {
    FetchBankError(reqwest::Error),
    SomethingWrong,
}

#[derive(Debug)]
struct Bank {
    name: String,
    phonetic: String,
    code: String,
    search_key: String,
}

impl Bank {
    fn fetch_branches(&self) {

    }
}

struct Branch {
    name: String,
    phonetic: String,
    code: String,
}

impl Branch {
    fn fetch(&self) {

    }
}

async fn fetch_bank_list(client: Client, head_char: char) -> Result<Vec<Bank>, Error> {
    let html = client
        .post("https://zengin.ajtw.net/ginkou.php")
        .form(&[("gm", &head_char.to_string())])
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
            let search_key = datarows
                .next()
                .unwrap()
                .find(Name("button"))
                .next()
                .unwrap()
                .attr("value")
                .unwrap()
                .to_owned();
            Bank {
                name,
                phonetic,
                code,
                search_key,
            }
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

#[tokio::main]
async fn main() {
    let client = Client::new();
    let search_keys = "あい".chars();
    let result = fetch_all_bank_list(client, search_keys).await;
    println!("done");
}
