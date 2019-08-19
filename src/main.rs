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
use data::{Config, MRRequest, ProjectResponse};
use failure::Error;
use futures::future;
use git2::{PushOptions, RemoteCallbacks, Repository};
use hyper::rt::{self, Future};
use std::env;
use std::fs::{self, File};
use std::io::prelude::*;
use std::io::BufReader;

mod data;
mod http;

const SECRETS_FILE: &str = "./.secret";
const CONFIG_FILE: &str = "./config.toml";

fn git_credentials_callback(
    _user: &str,
    user_from_url: Option<&str>,
    cred: git2::CredentialType,
) -> Result<git2::Cred, git2::Error> {
    let user = user_from_url.unwrap_or("git");

    if cred.contains(git2::CredentialType::USERNAME) {
        return git2::Cred::username(user);
    }
    let key_file = env::var("SSH_KEY_FILE").expect("no ssh key file provided");
    let passphrase = env::var("SSH_PASS").expect("no ssh pass provided");
    git2::Cred::ssh_key(
        user,
        None,
        std::path::Path::new(&key_file),
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
    Ok(config)
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
            acc
        },
    )
}

fn create_mr(
    config: Config,
    actual_remote: String,
    access_token: String,
    title: String,
    description: String,
    target_branch: String,
    current_branch: String,
) {
    let fut = http::fetch_projects(config.clone(), access_token.clone(), "projects".to_string())
        .and_then(move |projects: Vec<ProjectResponse>| {
            let mut actual_project: Option<&ProjectResponse> = None;
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
                access_token: access_token,
                project,
                title: title,
                description: description,
                source_branch: current_branch,
                target_branch: target_branch,
            };
            http::create_mr(&mr_req, &config)
        })
        .map_err(|e| {
            println!("Could not create MR, Error: {}", e);
        })
        .and_then(|url: String| {
            println!("Pushed and Created MR Successfully - URL: {}", url);
            future::ok(())
        });
    rt::run(fut);
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
    let actual_remote = String::from(remote.url().expect("could not get remote URL"));
    let remote_clone = actual_remote.clone();
    let branch_clone = current_branch.clone();
    callbacks.credentials(git_credentials_callback);
    callbacks.push_update_reference(move |refname, _| {
        println!("Successfully Pushed: {:?}", refname);
        create_mr(
            config.clone(),
            remote_clone.clone(),
            access_token.clone(),
            title.to_owned(),
            description.to_owned(),
            target_branch.to_owned(),
            branch_clone.clone(),
        );
        Ok(())
    });
    push_opts.remote_callbacks(callbacks);
    remote
        .push(
            &[&format!("refs/heads/{}", current_branch.to_string())],
            Some(&mut push_opts),
        )
        .expect("could not push to origin");
}
