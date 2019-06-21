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

use clap::{App, Arg, SubCommand};
use failure::Error;
use futures::future;
use git2::{PushOptions, RemoteCallbacks, Repository, Status};
use hyper::client::HttpConnector;
use hyper::rt::{self, Future, Stream};
use hyper::{Body, Client, Request, Response};
use hyper_tls::HttpsConnector;
use std::collections::HashMap;
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
}

#[derive(Debug, Deserialize, Clone)]
struct MRConfig {
    labels: Option<Vec<String>>,
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
) -> impl Future<Item = Vec<data::ProjectResponse>, Error = ()> {
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
        .map_err(|err| {
            println!("Error: {}", err);
        })
}

fn fetch_merge_requests(
    config: Config,
    access_token: String,
    domain: String,
) -> impl Future<Item = Vec<data::MergeRequestResponse>, Error = ()> {
    fetch(config, access_token, domain, 20)
        .and_then(|bodies| {
            let mut result: Vec<data::MergeRequestResponse> = Vec::new();
            for b in bodies {
                let bytes = b.concat2().wait().unwrap().into_bytes(); // TODO: necessary? handle unwrap nicer
                                                                      // https://github.com/hyperium/hyper/blob/e61fe540932c2e79ccabe3340e1471e357649e3c/examples/echo.rs
                                                                      // https://github.com/hyperium/hyper/blob/271bba16672ff54a44e043c5cc1ae6b9345bb172/src/client/mod.rs#L51
                let mut data: Vec<data::MergeRequestResponse> =
                    serde_json::from_slice(&bytes).unwrap();
                result.append(&mut data);
            }
            return future::ok(result);
        })
        .map_err(|err| {
            println!("Error: {}", err);
        })
}

fn fetch(
    config: Config,
    access_token: String,
    domain: String,
    per_page: i32,
) -> impl Future<Item = Vec<Body>, Error = hyper::Error> {
    // TODO: handle 400 / 500 errors as errors
    let https = HttpsConnector::new(4).unwrap();
    let client = Client::builder().build::<_, hyper::Body>(https);
    let group = config.group.as_ref().expect("group not configured");
    let req = Request::builder()
        .uri(format!(
            "https://gitlab.com/api/v4/groups/{}/{}?per_page={}",
            group, domain, per_page
        ))
        .header("PRIVATE-TOKEN", access_token.to_owned())
        .body(Body::empty())
        .unwrap();
    client.request(req).and_then(move |res: Response<Body>| {
        let mut result: Vec<Body> = Vec::new();
        let pages: &str = match res.headers().get("x-total-pages") {
            Some(v) => match v.to_str() {
                Ok(v) => v,
                _ => "0",
            },
            None => "0",
        };
        let p = pages.parse::<i32>().unwrap();
        let body: Body = res.into_body();
        result.push(body);
        let mut futrs = Vec::new();
        for page in 2..=p {
            futrs.push(fetch_paged(&config, &access_token, &domain, &client, page));
        }
        return future::join_all(futrs).and_then(move |bodies| {
            for b in bodies {
                result.push(b);
            }
            return future::ok(result);
        });
    })
}

fn fetch_paged(
    config: &Config,
    access_token: &str,
    domain: &str,
    client: &hyper::Client<HttpsConnector<HttpConnector>>,
    page: i32,
) -> impl Future<Item = Body, Error = hyper::Error> {
    let group = config.group.as_ref().expect("group not configured");
    let req = Request::builder()
        .uri(format!(
            "https://gitlab.com/api/v4/groups/{}/{}?per_page=20&page={}",
            group, domain, page
        ))
        .header("PRIVATE-TOKEN", access_token)
        .body(Body::empty())
        .unwrap();
    client.request(req).and_then(|res| {
        let body = res.into_body();
        future::ok(body)
    })
}

fn parse_filters(filters: &Vec<&str>) -> HashMap<String, String> {
    let mut result = HashMap::new();
    filters.iter().for_each(|f| {
        let parsed = parse_filter(f).unwrap(); // TODO: handle none case
        println!("parsed: {} {}", parsed.0, parsed.1);
        result.insert(parsed.0.to_owned(), parsed.1.to_owned());
    });
    return result;
}

fn parse_filter(filter: &str) -> Option<(&str, &str)> {
    // TODO: handle errors, wrong inputs
    let parts: Vec<&str> = filter.split('=').collect();
    return Some((parts.get(0).unwrap(), parts.get(1).unwrap()));
}

fn main() {
    println!("Hello, CLG!");
    let matches = App::new("CLG")
        .version("1.0")
        .author("Mario Zupan")
        .about("A GitLab Commandline Utility")
        .subcommand(
            SubCommand::with_name("list")
                .about("Shows a list")
                .arg(
                    Arg::with_name("filters")
                        .short("f")
                        .long("filters")
                        .takes_value(true)
                        .use_delimiter(true)
                        .multiple(true)
                        .help("filters for entries"),
                )
                .arg(
                    Arg::with_name("projects")
                        .short("p")
                        .long("projects")
                        .help("list projects"),
                )
                .arg(
                    Arg::with_name("merge-requests")
                        .short("m")
                        .long("mr")
                        .help("list merge-requests"),
                ),
        )
        .subcommand(
            SubCommand::with_name("push-and-mr")
                .about(
                    "Pushes and creates a Merge-Request automatically based on a predefined config",
                )
                .arg(
                    Arg::with_name("description")
                        .short("d")
                        .long("description")
                        .help("the description for the MR"),
                ),
        )
        .get_matches();
    let access_token = get_access_token().expect("could not get access token");
    let config = get_config().expect("could not read config");
    let mr_config = get_mr_config().expect("could not read merge-request config");

    if let Some(matches) = matches.subcommand_matches("list") {
        if matches.is_present("filters") {
            let filters = matches.values_of("filters").unwrap().collect::<Vec<_>>();
            let parsed_filters = parse_filters(&filters);
            println!("filters: {:?}", parsed_filters);
        }
        if matches.is_present("projects") {
            println!("Listing Projects:");
            let fut = fetch_projects(config, access_token, "projects".to_string()).and_then(
                |projects: Vec<data::ProjectResponse>| {
                    for p in &projects {
                        println!("{:?}", p);
                    }
                    future::ok(())
                },
            );
            rt::run(fut);
        } else if matches.is_present("merge-requests") {
            println!("Listing Merge Requests:");
            let fut = fetch_merge_requests(config, access_token, "merge_requests".to_string())
                .and_then(|merge_requests: Vec<data::MergeRequestResponse>| {
                    for mr in &merge_requests {
                        println!("{:?}", mr);
                    }
                    future::ok(())
                });
            rt::run(fut);
        } else {
            println!("You have to specify what to list")
        }
    } else if let Some(_) = matches.subcommand_matches("push-and-mr") {
        println!("Pushing and Creating Merge Request:");
        println!("With config: {:?}", mr_config);
        let repo = Repository::open("./").expect("current folder is not a git repository");
        let mut current_branch: String = "".to_string();
        let branches = repo.branches(None).expect("can't list branches");
        branches.for_each(|branch| {
            let b = branch.unwrap();
            println!("branch: {:?}", b.0.name());
            println!("current: {:?}", b.0.is_head());
            println!("remote or local: {:?}", b.1);
            if b.0.is_head() {
                current_branch =
                    b.0.name()
                        .expect("cant set current branch")
                        .unwrap()
                        .to_string(); // TODO: handle errors
            }
        });

        let remotes = repo.remotes().expect("can't list remotes");
        let mut actual_remote: String = "".to_string();
        remotes.iter().for_each(|remote| {
            let rm = remote.unwrap();
            if rm == "origin" {
                println!("remote: {:?}", rm);
            }
            println!("branch: {:?}", current_branch);
            actual_remote = String::from(
                repo.find_remote(rm)
                    .expect("cant find remote")
                    .url()
                    .unwrap(),
            );
        });
        println!("actual remote: {:?}", actual_remote);
        let repo_url = String::from(actual_remote);

        let project_future = fetch_projects(config, access_token, "projects".to_string()).and_then(
            move |projects: Vec<data::ProjectResponse>| {
                for p in &projects {
                    if p.ssh_url_to_repo == repo_url {
                        println!("Actual Project: {:?}", p);
                    }
                    if p.http_url_to_repo == repo_url {
                        println!("Actual Project: {:?}", p);
                    }
                }
                // TODO: with current branch and repo url, create merge request
                // TODO: move project to gitlab to try it out
                future::ok(())
            },
        );
        let result = rt::run(project_future);
        println!("result: {:?}", result); // TODO: return result here
                                          // TODO: fetch projects, THEN check if ssh_url_to_repo or http_url_to_repo match
                                          // then, using remotes, select current project for creating an MR
        let statuses = repo
            .statuses(None)
            .expect("could not get git status of repository");
        println!("git status: {:?}", statuses.len());
        for i in 0..statuses.len() {
            let status = statuses.get(i).expect("could not get status");
            if status.status() == Status::WT_MODIFIED {
                println!("MODIFIED: git status: {:?}", status.status());
                println!("git path: {:?}", status.path().unwrap_or("fail"));
            }
        }
        let mut revwalk = repo.revwalk().expect("can't iterate revisions");
        // TODO: log => https://github.com/rust-lang/git2-rs/blob/master/examples/log.rs
        println!("# iterating commits");
        revwalk.push_head().expect("cant push head to revwalk");
        // revwalk.for_each(|id| {
        //     let oid = id.unwrap();
        //     println!("oid: {:?}", oid);
        //     let commit = repo.find_commit(oid).expect("can't find commit for oid");
        //     println!("commit: {:?}", commit);
        //     println!("commit message: {:?}", commit.message());
        //     println!("commit: {:?}", commit.message());
        //     let tree = commit.tree().expect("can't fetch tree for commit");
        //     println!("tree: {:?}", tree);
        // });
        println!("");
        println!("");
        println!("");
        println!("");
        let mut callbacks = RemoteCallbacks::new();
        callbacks.credentials(git_credentials_callback);
        callbacks.push_update_reference(|refname, status| {
            println!("updated refname: {:?}", refname);
            println!("updated status: {:?}", status);
            // TODO: create merge request in gitlab
            // https://docs.gitlab.com/ee/api/merge_requests.html#create-mr
            Ok(())
        });
        let mut push_opts = PushOptions::new();
        push_opts.remote_callbacks(callbacks);
        // let remotes = repo.remotes().expect("can't list remotes");
        // remotes.iter().for_each(|remote| {
        //     let rm = remote.unwrap();
        //     if rm == "origin" {
        //         println!("remote: {:?}", rm);
        //     }
        //     println!("branch: {:?}", current_branch);
        //     let mut actual_remote = repo.find_remote(rm).expect("cant find remote");
        //     // TODO: find out current project by using remote URL!
        //     println!("actual remote: {:?}", actual_remote.url());
        //     // println!("Current Repository: {:?}", repo)
        //     actual_remote
        //         .push(
        //             &[&format!("refs/heads/{}", current_branch.to_string())],
        //             Some(&mut push_opts),
        //         )
        //         .expect("could not push to origin");
        // });
        // TODO: if files are INDEX_MODIFIED, notify that stuff is unstaged
        // TODO: make sure mr.toml is read and there is a merge-request config
        // also, ask before executing, showing a summary before
        // then, push the changes, and using the MR ID from the response, create an MR based on a
        // mr.toml file using the API
    }
}
