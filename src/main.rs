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
use std::fs::{self};

mod data;
mod http;

const CONFIG_FILE: &str = ".glpm/config.toml";

fn git_credentials_ssh_callback(
    _user: &str,
    user_from_url: Option<&str>,
    cred: git2::CredentialType,
) -> Result<git2::Cred, git2::Error> {
    let user = user_from_url.unwrap_or("git");

    if cred.contains(git2::CredentialType::USERNAME) {
        return git2::Cred::username(user);
    }
    let config = get_config().expect("Could not read config");
    let key_file = &config.ssh_key_file.unwrap();
    let passphrase = &config.ssh_passphrase.unwrap();
    git2::Cred::ssh_key(
        user,
        None,
        std::path::Path::new(&key_file),
        Some(&passphrase),
    )
}

fn git_credentials_pwd_callback(
    _user: &str,
    _user_from_url: Option<&str>,
    _cred: git2::CredentialType,
) -> Result<git2::Cred, git2::Error> {
    let config = get_config().expect("Could not read config");
    git2::Cred::userpass_plaintext(&config.user.unwrap(), &config.password.unwrap())
}

fn get_config() -> Result<Config, Error> {
    let config_file: &str =
        &(env::var("HOME").expect("Cannot find HOME environment variable") + "/" + CONFIG_FILE);

    let data = fs::read_to_string(config_file)?;
    let config: Config = toml::from_str(&data)?;
    Ok(config)
}

fn get_current_branch(repo: &Repository) -> Result<String, Error> {
    let branches = repo.branches(None).expect("can't list branches");
    branches.fold(
        Err(format_err!("Couldn't find current branch")),
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
                access_token,
                project,
                title,
                description,
                source_branch: current_branch,
                target_branch,
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

    let config = get_config().expect("Could not read config file");
    let access_token = config
        .clone()
        .apikey
        .expect("Could not get access token")
        .to_string();

    if config.group.is_none() && config.user.is_none() {
        panic!("Group or User for Gitlab need to be configured")
    }

    let repo = Repository::open("./").expect("Current folder is not a git repository");
    let current_branch = get_current_branch(&repo).expect("Could not get current branch");
    let mut remote = repo
        .find_remote("origin")
        .expect("Origin remote could not be found");

    let mut push_opts = PushOptions::new();
    let mut callbacks = RemoteCallbacks::new();
    let actual_remote = String::from(remote.url().expect("Could not get remote URL"));
    let remote_clone = actual_remote.clone();
    let branch_clone = current_branch.clone();
    if config.password.is_none() {
        callbacks.credentials(git_credentials_ssh_callback);
    } else {
        callbacks.credentials(git_credentials_pwd_callback);
    }
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
        .expect("Could not push to origin");
}
