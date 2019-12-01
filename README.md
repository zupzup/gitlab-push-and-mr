# gitlab-push-and-mr

Push and create an MR automatically using gitlab API and GIT.

You need a Gitlab account and a project there, plus an API key.

# Run

All parameters must be configured into $HOME/.glpm/config.toml file:

```toml
user = "user_name"
password = "user_password"
ssh_key_file = "/home/user_name/.ssh/id_rsa"
ssh_passphrase = "user_passphrase"
apikey="gitlab_api_key"
mr_labels = ["DevOps"]
host = "http://gitlab.example.com"
```

If password key is defined, user and password will be used to perform the authentication. Otherwise, it will use ssh private key and passphare configuration.

Execute

```bash
// run tool
cargo run -- -d "Some Description" -t "Some Title"
```
