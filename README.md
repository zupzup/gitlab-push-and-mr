# gitlab-push-and-mr

Push and create an MR automatically using gitlab API and GIT.

You need a Gitlab account and a project there, plus an API key.

# Run

With your Gitlab API secret in a `./secret` file and an example Config.toml file:

```toml
user = "mzupanmz"
mr_labels = ["Waiting for Review"]
```

Execute:

```bash
// set ssh config to environment
export SSH_KEY_FILE=/path/to/private/key
export SSH_PASS=my_password

// run tool
cargo run -- -d "Some Description" -t "Some Title"
```
