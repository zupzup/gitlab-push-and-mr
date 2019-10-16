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
use error::AppError;
use git2::{PushOptions, RemoteCallbacks, Repository};
use std::env;
use std::fs::{self, File};
use std::io::prelude::*;
use std::io::BufReader;
use tokio::runtime::Runtime;

mod data;
mod error;
mod http;

const SECRETS_FILE: &str = "./.secret";
const CONFIG_FILE: &str = "./config.toml";

type Result<T> = std::result::Result<T, AppError>;

fn git_credentials_callback(
    _user: &str,
    user_from_url: Option<&str>,
    cred: git2::CredentialType,
) -> std::result::Result<git2::Cred, git2::Error> {
    let user = user_from_url.unwrap_or("git");

    if cred.contains(git2::CredentialType::USERNAME) {
        return git2::Cred::username(user)
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

fn get_access_token() -> Result<String> {
    let file = File::open(SECRETS_FILE).expect("Could not read access token file");
    let buf = BufReader::new(file);
    let lines: Vec<String> = buf
        .lines()
        .take(1)
        .map(std::result::Result::unwrap_or_default)
        .collect();
    if lines[0].is_empty() {
        return Err(AppError::AccessTokenNotFoundError());
    }
    Ok(lines[0].to_string())
}

fn get_config() -> Result<Config> {
    let data = fs::read_to_string(CONFIG_FILE)?;
    let config: Config = toml::from_str(&data)?;
    Ok(config)
}

fn get_current_branch(repo: &Repository) -> Result<String> {
    let branches = repo.branches(None).expect("can't list branches");
    branches.fold(
        Err(AppError::GitError(String::from("current branch not found"))),
        |acc, branch| {
            let b =
                branch.map_err(|_| AppError::GitError(String::from("current branch not found")))?;
            if b.0.is_head() {
                let name = b
                    .0
                    .name()
                    .map_err(|_| AppError::GitError(String::from("current branch not found")))?;
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
    config: &Config,
    actual_remote: &str,
    access_token: &str,
    title: &str,
    description: &str,
    target_branch: &str,
    current_branch: &str,
) {
    let rt = Runtime::new().expect("tokio runtime can be initialized");
    rt.block_on(async move {
        let projects = match http::fetch_projects(&config, &access_token, "projects").await {
            Ok(v) => v,
            Err(e) => return println!("could not fetch projects, reason: {}", e)
        };
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
            access_token,
            project,
            title,
            description,
            source_branch: current_branch,
            target_branch,
        };
        match http::create_mr(&mr_req, &config).await {
            Ok(v) => println!("Pushed and Created MR Successfully - URL: {}", v),
            Err(e) => println!("Could not create MR, Error: {}", e)
        };
    });
}

fn main() -> Result<()> {
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
    let branch_clone = current_branch.clone();
    callbacks.credentials(git_credentials_callback);
    callbacks.push_update_reference(move |refname, _| {
        println!("Successfully Pushed: {:?}", refname);
        create_mr(
            &config,
            &actual_remote,
            &access_token,
            &title,
            &description,
            &target_branch,
            &branch_clone,
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
    Ok(())
}
