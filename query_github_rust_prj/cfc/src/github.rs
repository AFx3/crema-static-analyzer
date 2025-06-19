use anyhow::Result;
use reqwest::{Client, header::{HeaderMap, AUTHORIZATION, USER_AGENT, ACCEPT}};
use serde_derive::Deserialize;
use std::time::Duration;
use tokio::time::{sleep, interval, Interval};



#[derive(Deserialize)]
struct CodeSearchItem { repository: RepoRef }
#[derive(Deserialize)]
struct RepoRef { full_name: String }
#[derive(Deserialize)]
struct CodeSearch1 { items: Vec<CodeSearchItem> }




// search in a single query all main.rs that contain unsafe
pub async fn search_rust_main_with_unsafe() -> Result<Vec<String>> {
    // 1) client GitHub con header
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, "ffi-main-checker".parse()?);
    headers.insert(ACCEPT, "application/vnd.github.v3+json".parse()?);
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        headers.insert(AUTHORIZATION, format!("token {}", token).parse()?);
    } else {
        eprintln!("⚠️ GITHUB_TOKEN non settato; attenzione ai rate limit");
    }
    let gh = Client::builder()
        .default_headers(headers)
        .build()?;

    // 2) build the query
    //    - filename:main.rs
    //    - path:src
    //    - language:Rust
    //    - unsafe
    let q = "filename:main.rs path:src language:Rust unsafe";
    let repos = paged_code_search_repos(&gh, q).await?;

    println!("Found {} repository having src/main.rs relying on `unsafe`", repos.len());
    Ok(repos)
}

async fn paged_code_search_repos(gh: &Client, query: &str) -> Result<Vec<String>> {
    let mut unique = std::collections::HashSet::new();
    let mut all_repos = Vec::new();
    let per_page = 100;
    let mut page = 1;

    // use ticker to not surpass 30 req/min
    let mut ticker: Interval = interval(Duration::from_secs(2));

    loop {
        ticker.tick().await;  // <= 1 req every 2s

        let url = format!("https://api.github.com/search/code?q={}&per_page={}&page={}", urlencoding::encode(query), per_page, page);
        let resp = gh.get(&url).send().await?;

        // back‐off on 403 (forbidden) + Retry‐After
        let resp = if resp.status() == reqwest::StatusCode::FORBIDDEN {
            if let Some(retry) = resp.headers().get("Retry-After") {
                if let Ok(secs) = retry.to_str()
                                       .and_then(|s| Ok(s.parse::<u64>().map_err(|_| ()))) {
                    eprintln!("Rate limit, waitning {:?}s…", secs);
                    sleep(Duration::from_secs(secs.expect("nisba"))).await;
                    gh.get(&url).send().await?
                } else { resp }
            } else { resp }
        } else { resp };

        if !resp.status().is_success() {
            eprintln!("GitHub search page failed {}: {}", page, resp.status());
            break;
        }

        let CodeSearch1 { items } = resp.json().await?;
        if items.is_empty() {
            break;
        }

        for it in &items {
            let repo = it.repository.full_name.clone();
            if unique.insert(repo.clone()) {
                all_repos.push(repo);
            }
        }

        if items.len() < per_page {
            break; // last apge
        }
        page += 1;
    }

    Ok(all_repos)
}

pub async fn search_rust_main_with_unsafe_ops() -> Result<Vec<String>> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, "ffi-main-checker".parse()?);
    headers.insert(ACCEPT, "application/vnd.github.v3+json".parse()?);
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        headers.insert(AUTHORIZATION, format!("token {}", token).parse()?);
    }
    let gh = Client::builder().default_headers(headers).build()?;

    // 2) query:
    //    - filename:main.rs
    //    - path:src
    //    - language:Rust
    //    - std::mem::forget()
    let q = r#"language:Rust "Box::into_raw""#;
   
  



    let repos = paged_code_search_repos(&gh, q).await?;

    println!("found {} repository with src/main.rs",repos.len());
    Ok(repos)
}
