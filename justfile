# Gateway build and deploy.
# The release binary embeds gateway-admin/dist. Build the admin UI before cargo.

binary := justfile_directory() / "target" / "release" / "superglue"
features := "gateway,capture"

# List recipes
[private]
default:
    @just --list

# Run a recipe from this directory. Same names as `just superglue` at the repo root.
superglue *args:
    just --justfile "{{justfile()}}" --working-directory "{{justfile_directory()}}" {{args}}

# Build the admin UI. The gateway binary embeds gateway-admin/dist.
admin:
    npm ci --prefix "{{justfile_directory()}}/gateway-admin"
    npm run build --prefix "{{justfile_directory()}}/gateway-admin"

# Build the release binary. Rebuilds the admin UI first.
build: admin
    cargo build --release --features {{features}} --manifest-path "{{justfile_directory()}}/Cargo.toml" --bin superglue

# Install the current binary on the existing host. Rebuilds the admin UI first.
redeploy *args: build
    SUPERGLUE_LOCAL_BINARY="{{binary}}" "{{justfile_directory()}}/deploy/setup-server.sh" {{args}}

# Create the host, DNS, and gateway. Rebuilds the admin UI first.
provision: build
    SUPERGLUE_LOCAL_BINARY="{{binary}}" "{{justfile_directory()}}/deploy/deploy.sh"
