use crate::data::{Config, MRPayload, MRRequest, MRResponse, ProjectResponse};
use failure::Error;
use futures::{future, stream::Concat2};
use hyper::client::HttpConnector;
use hyper::rt::{Future, Stream};
use hyper::{Body, Client, Method, Request, Response};
use hyper_tls::HttpsConnector;

pub fn fetch_projects(
    config: Config,
    access_token: String,
    domain: String,
) -> impl Future<Item = Vec<ProjectResponse>, Error = Error> {
    fetch(config, access_token, domain, 20)
        .and_then(|bodies| {
            let mut result: Vec<ProjectResponse> = Vec::new();
            future::join_all(bodies)
                .and_then(|bods| {
                    for b in bods {
                        let bytes = b.into_bytes();
                        let mut data: Vec<ProjectResponse> = serde_json::from_slice(&bytes)
                            .expect("can't parse data to project response");
                        result.append(&mut data);
                    }
                    future::ok(result)
                })
                .map_err(|e| format_err!("req err: {}", e))
        })
        .map_err(|err| err)
}

fn fetch(
    config: Config,
    access_token: String,
    domain: String,
    per_page: i32,
) -> impl Future<Item = Vec<Concat2<Body>>, Error = Error> {
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
            future::ok(res)
        })
        .and_then(move |res: Response<Body>| {
            let mut result: Vec<Concat2<Body>> = Vec::new();
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
            result.push(body.concat2());
            let mut futrs = Vec::new();
            for page in 2..=p {
                futrs.push(fetch_paged(&config, &access_token, &domain, &client, page));
            }
            future::join_all(futrs)
                .and_then(move |bodies| {
                    for b in bodies {
                        result.push(b);
                    }
                    future::ok(result)
                })
                .map_err(|e| format_err!("requests error: {}", e))
        })
}

fn fetch_paged(
    config: &Config,
    access_token: &str,
    domain: &str,
    client: &hyper::Client<HttpsConnector<HttpConnector>>,
    page: i32,
) -> impl Future<Item = Concat2<Body>, Error = Error> {
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
            let body = res.into_body().concat2();
            future::ok(body)
        })
}

pub fn create_mr(
    payload: &MRRequest,
    config: &Config,
) -> impl Future<Item = String, Error = Error> {
    let https = HttpsConnector::new(4).expect("https connector works");
    let client = Client::builder().build::<_, hyper::Body>(https);
    let uri = format!(
        "https://gitlab.com/api/v4/projects/{}/merge_requests",
        payload.project.id
    );
    let labels = config
        .mr_labels
        .as_ref()
        .unwrap_or(&Vec::new())
        .iter()
        .fold(String::new(), |acc, l| format!("{}, {}", acc, l));

    let mr_payload = MRPayload {
        id: format!("{}", payload.project.id),
        title: payload.title.clone(),
        description: payload.description.clone(),
        target_branch: payload.target_branch.clone(),
        source_branch: payload.source_branch.clone(),
        labels,
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
            body.concat2()
                .map_err(|e| format_err!("requests error: {}:", e))
        })
        .and_then(|body| {
            let bytes = body.into_bytes();
            let data: MRResponse =
                serde_json::from_slice(&bytes).expect("can't parse data to merge request response");
            future::ok(data.web_url)
        })
        .map_err(|e| format_err!("requests error: {}", e))
}
