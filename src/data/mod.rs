use serde_json::Number;

#[derive(Serialize, Deserialize, Debug)]
pub struct GroupResponse {
    pub id: Number,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ProjectResponse {
    pub id: Number,
    pub name: String,
    pub ssh_url_to_repo: String,
    pub http_url_to_repo: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MergeRequestResponse {
    pub id: Number,
    pub title: String,
    pub author: Author,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Author {
    pub id: Number,
    pub name: String,
    pub username: String,
}

#[derive(Debug, Deserialize)]
pub struct MRResponse {
    pub web_url: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct MRPayload {
    pub id: String,
    pub title: String,
    pub description: String,
    pub source_branch: String,
    pub target_branch: String,
    pub labels: String,
    pub remove_source_branch: bool,
    pub squash: bool,
}
