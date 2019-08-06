#[macro_use]
extern crate failure;
extern crate hyper;
extern crate hyper_tls;
#[macro_use]
extern crate serde_derive;
extern crate clap;
extern crate futures;
extern crate serde;
extern crate serde_json;
extern crate toml;

use clap::{App, Arg};
use failure::Error;
use futures::future;
use git2::{PushOptions, RemoteCallbacks, Repository};
use hyper::client::HttpConnector;
use hyper::rt::{self, Future, Stream};
use hyper::{Body, Client, Method, Request, Response};
use hyper_tls::HttpsConnector;
use std::env;
use std::fs::{self, File};
use std::io::prelude::*;
use std::io::BufReader;

mod data;

const SECRETS_FILE: &str = "./.secret";
const CONFIG_FILE: &str = "./.config.toml";
const MR_CONFIG_FILE: &str = "./mr.toml";
const SSH_KEY_FILE: &str = "/Users/mario/.ssh/id_rsa";

#[derive(Debug, Deserialize, Clone)]
struct Config {
    group: Option<String>,
    user: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct MRConfig {
    labels: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Clone)]
struct MRRequest<'a> {
    access_token: String,
    project: &'a data::ProjectResponse,
    title: String,
    description: String,
    source_branch: String,
    target_branch: String,
}

fn git_credentials_callback(
    _user: &str,
    user_from_url: Option<&str>,
    cred: git2::CredentialType,
) -> Result<git2::Cred, git2::Error> {
    let user = user_from_url.unwrap_or("git");

    if cred.contains(git2::CredentialType::USERNAME) {
        return git2::Cred::username(user);
    }
    let passphrase = env::var("SSH_PASS").expect("no ssh pass provided");
    git2::Cred::ssh_key(
        user,
        None,
        std::path::Path::new(SSH_KEY_FILE),
        Some(&passphrase),
    )
}

fn get_access_token() -> Result<String, Error> {
    let file = File::open(SECRETS_FILE).expect("Could not read access token file");
    let buf = BufReader::new(file);
    let lines: Vec<String> = buf
        .lines()
        .take(1)
        .map(std::result::Result::unwrap_or_default)
        .collect();
    if lines[0].is_empty() {
        return Err(format_err!("access token mustn't be empty"));
    }
    Ok(lines[0].to_string())
}

fn get_config() -> Result<Config, Error> {
    let data = fs::read_to_string(CONFIG_FILE)?;
    let config: Config = toml::from_str(&data)?;
    return Ok(config);
}

fn get_mr_config() -> Result<MRConfig, Error> {
    let data = fs::read_to_string(MR_CONFIG_FILE)?;
    let config: MRConfig = toml::from_str(&data)?;
    return Ok(config);
}

fn fetch_projects(
    config: Config,
    access_token: String,
    domain: String,
) -> impl Future<Item = Vec<data::ProjectResponse>, Error = Error> {
    fetch(config, access_token, domain, 20)
        .and_then(|bodies| {
            let mut result: Vec<data::ProjectResponse> = Vec::new();
            for b in bodies {
                let bytes = b.concat2().wait().unwrap().into_bytes();
                let mut data: Vec<data::ProjectResponse> =
                    serde_json::from_slice(&bytes).expect("can't parse data to project response");
                result.append(&mut data);
            }
            return future::ok(result);
        })
        .map_err(|err| err)
}

fn fetch(
    config: Config,
    access_token: String,
    domain: String,
    per_page: i32,
) -> impl Future<Item = Vec<Body>, Error = Error> {
    let https = HttpsConnector::new(4).expect("https connector works");
    let client = Client::builder().build::<_, hyper::Body>(https);
    let group = config.group.as_ref();
    let user = config.user.as_ref();
    let uri = match group {
        Some(v) => format!(
            "https://gitlab.com/api/v4/groups/{}/{}?per_page={}",
            v, domain, per_page
        ),
        None => match user {
            Some(u) => format!(
                "https://gitlab.com/api/v4/users/{}/{}?per_page={}",
                u, domain, per_page
            ),
            None => "invalid url".to_string(),
        },
    };
    let req = Request::builder()
        .uri(uri)
        .header("PRIVATE-TOKEN", access_token.to_owned())
        .body(Body::empty())
        .expect("request creation works");
    client
        .request(req)
        .map_err(|e| format_err!("req err: {}", e))
        .and_then(move |res: Response<Body>| {
            if !res.status().is_success() {
                return future::err(format_err!("unsuccessful fetch request: {}", res.status()));
            }
            return future::ok(res);
        })
        .and_then(move |res: Response<Body>| {
            let mut result: Vec<Body> = Vec::new();
            let pages: &str = match res.headers().get("x-total-pages") {
                Some(v) => match v.to_str() {
                    Ok(v) => v,
                    _ => "0",
                },
                None => "0",
            };
            let p = match pages.parse::<i32>() {
                Ok(v) => v,
                Err(_) => 0,
            };
            let body: Body = res.into_body();
            result.push(body);
            let mut futrs = Vec::new();
            for page in 2..=p {
                futrs.push(fetch_paged(&config, &access_token, &domain, &client, page));
            }
            return future::join_all(futrs)
                .and_then(move |bodies| {
                    for b in bodies {
                        result.push(b);
                    }
                    return future::ok(result);
                })
                .map_err(|e| format_err!("requests error: {}", e));
        })
}

fn fetch_paged(
    config: &Config,
    access_token: &str,
    domain: &str,
    client: &hyper::Client<HttpsConnector<HttpConnector>>,
    page: i32,
) -> impl Future<Item = Body, Error = Error> {
    let group = config.group.as_ref().expect("group is configured");
    let req = Request::builder()
        .uri(format!(
            "https://gitlab.com/api/v4/groups/{}/{}?per_page=20&page={}",
            group, domain, page
        ))
        .header("PRIVATE-TOKEN", access_token)
        .body(Body::empty())
        .expect("request can be created");
    client
        .request(req)
        .map_err(|e| format_err!("req err: {}", e))
        .and_then(|res| {
            if !res.status().is_success() {
                return future::err(format_err!(
                    "unsuccessful fetch paged request: {}",
                    res.status()
                ));
            }
            let body = res.into_body();
            future::ok(body)
        })
}

fn create_mr(
    payload: &MRRequest,
    mr_config: &MRConfig,
) -> impl Future<Item = String, Error = Error> {
    let https = HttpsConnector::new(4).expect("https connector works");
    let client = Client::builder().build::<_, hyper::Body>(https);
    let uri = format!(
        "https://gitlab.com/api/v4/projects/{}/merge_requests",
        payload.project.id
    );
    let labels = mr_config
        .labels
        .as_ref()
        .unwrap_or(&Vec::new())
        .iter()
        .fold(String::new(), |acc, l| format!("{}, {}", acc, l));

    let mr_payload = data::MRPayload {
        id: format!("{}", payload.project.id),
        title: payload.title.clone(),
        description: payload.description.clone(),
        target_branch: payload.target_branch.clone(),
        source_branch: payload.source_branch.clone(),
        labels: labels,
        squash: true,
        remove_source_branch: true,
    };
    let json = serde_json::to_string(&mr_payload).expect("payload can be stringified");
    let req = Request::builder()
        .uri(uri)
        .header("PRIVATE-TOKEN", payload.access_token.to_owned())
        .header("Content-Type", "application/json")
        .method(Method::POST)
        .body(Body::from(json))
        .expect("request can be created");
    client
        .request(req)
        .map_err(|e| format_err!("req err: {}", e))
        .and_then(move |res: Response<Body>| {
            if !res.status().is_success() {
                return future::err(format_err!(
                    "unsuccessful create mr request: {}",
                    res.status()
                ));
            }
            let body = res.into_body();
            future::ok(body)
        })
        .and_then(|body: Body| {
            let bytes = body.concat2().wait().unwrap().into_bytes();
            let data: data::MRResponse =
                serde_json::from_slice(&bytes).expect("can't parse data to merge request response");
            return future::ok(data.web_url);
        })
        .map_err(|e| format_err!("requests error: {}", e))
}

fn get_current_branch(repo: &Repository) -> Result<String, Error> {
    let branches = repo.branches(None).expect("can't list branches");
    branches.fold(
        Err(format_err!("couldn't find current branch")),
        |acc, branch| {
            let b = branch?;
            if b.0.is_head() {
                let name = b.0.name()?;
                return match name {
                    Some(n) => Ok(n.to_string()),
                    None => return acc,
                };
            }
            return acc;
        },
    )
}

fn main() {
    let matches = App::new("Gitlab Push-and-MR")
        .version("1.0")
        .arg(
            Arg::with_name("description")
                .short("d")
                .long("description")
                .value_name("DESCRIPTION")
                .help("The Merge-Request description")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("title")
                .short("t")
                .required(true)
                .long("title")
                .value_name("TITLE")
                .help("The Merge-Request title")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("target_branch")
                .short("b")
                .long("target-branch")
                .value_name("TARGETBRANCH")
                .help("The Merge-Request target branch")
                .takes_value(true),
        )
        .get_matches();
    let title = matches
        .value_of("title")
        .expect("title needs to be provided");
    let description = matches.value_of("description").unwrap_or("");
    let target_branch = matches.value_of("target_branch").unwrap_or("master");

    let access_token = get_access_token().expect("could not get access token");
    let config = get_config().expect("could not read config");
    let mr_config = get_mr_config().expect("could not read merge-request config");

    if config.group.is_none() && config.user.is_none() {
        panic!("Group or User for Gitlab need to be configured")
    }

    let repo = Repository::open("./").expect("current folder is not a git repository");
    let current_branch = get_current_branch(&repo).expect("could not get current branch");
    let mut remote = repo
        .find_remote("origin")
        .expect("origin remote could not be found");

    let mut push_opts = PushOptions::new();
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(git_credentials_callback);
    callbacks.push_update_reference(|refname, _| {
        println!("Successfully Pushed: {:?}", refname);
        Ok(())
    });
    push_opts.remote_callbacks(callbacks);
    remote
        .push(
            &[&format!("refs/heads/{}", current_branch.to_string())],
            Some(&mut push_opts),
        )
        .expect("could not push to origin");
    let actual_remote = String::from(remote.url().expect("could not get remote URL"));

    let mr_access_token = access_token.clone();

    let title_clone = title.to_owned();
    let desc_clone = description.to_owned();
    let target_branch_clone = target_branch.to_owned();

    let fut = fetch_projects(config, access_token, "projects".to_string())
        .and_then(move |projects: Vec<data::ProjectResponse>| {
            let mut actual_project: Option<&data::ProjectResponse> = None;
            for p in &projects {
                if p.ssh_url_to_repo == actual_remote {
                    actual_project = Some(p);
                    break;
                }
                if p.http_url_to_repo == actual_remote {
                    actual_project = Some(p);
                    break;
                }
            }
            let project = actual_project.expect("couldn't find this project on gitlab");
            let mr_req = MRRequest {
                access_token: mr_access_token,
                project: project,
                title: title_clone,
                description: desc_clone,
                source_branch: current_branch,
                target_branch: target_branch_clone,
            };
            return create_mr(&mr_req, &mr_config);
        })
        .map_err(|e| {
            println!("Could not create MR, Error: {}", e);
        })
        .and_then(|url: String| {
            println!("Pushed and Created MR Successfully - URL: {}", url);
            return future::ok(());
        });
    rt::run(fut);
}
