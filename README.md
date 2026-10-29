# AMEISE adviser

A local web companion for the AMEISE QA 200 simulation. It collects observations from the desktop client, displays feedback, and can run an AI adviser. You choose and submit actions in the desktop client.

## Prerequisites

Run the commands below from the repository root.

The included Nix flake provides Rust, Cargo, a C compiler, and JDK 8 on `x86_64-linux`. With Nix and flakes enabled, enter the development shell:

```sh
nix develop
```

Repeat this in each terminal used below. Alternatively, install a current stable Rust toolchain with Cargo and a C build toolchain yourself. The desktop client and observer build also require **JDK 8** (`java` and `javac`); the patch tool uses JDK 8's bundled ASM.

For a live simulation, obtain the AMEISE desktop client distribution separately and place it in `Client-engine4/`, including `Client-engine4/ClientV50.jar` and its supplied runtime/configuration files. This directory is ignored by Git and is **not included in a clone**. You also need access to your AMEISE server and an account. The dashboard demo works without these.

## Start the dashboard

In the first terminal:

```sh
./adviser/run.sh
```

Open **http://127.0.0.1:8765** in a browser. Click **Load demo** to try synthetic events and example advice, or **Import packets** to load a recording. No simulator connection or API key is needed for the demo.

The launcher builds the Rust service using the committed Cargo lockfile; the first build needs network access to download dependencies. Dashboard assets are already tracked in `adviser/dist/`, so there is no frontend build step. Events and advice persist in `adviser/.runtime/adviser.sqlite3` by default.

## Connect the desktop client

Keep the dashboard service running. In a second terminal:

```sh
./launch_observed_client.sh
```

On its first run, this builds `Client-engine4/ClientV50-observer.jar` from the original JAR. Log in and select your game in the desktop client, then view the live stream in the dashboard. The observer sends events to `http://127.0.0.1:8765/api/v2/events` by default.

To launch the original client without observation instead:

```sh
./launch_client.sh
```

After changing the observer code or replacing the original client JAR, rebuild the patched JAR explicitly:

```sh
./adviser/client/build.sh
```

## Start the AI adviser (optional)

Keep the dashboard service and observed client running. In a third terminal, set your API credentials and a model available to your account that supports Responses API JSON mode:

```sh
export OPENAI_API_KEY='your-api-key'
export OPENAI_MODEL='your-model-id'
./adviser/run-agent.sh
```

The runner watches for changed game context, requests advice from OpenAI, and posts it to the dashboard. This sends the observed context to OpenAI and uses your API account. Starting the dashboard alone does not start the runner. Read the advice in the browser and apply chosen actions manually in the desktop client.

Use Ctrl-C in each service terminal to stop it, and close the desktop client when finished.

## Configuration

To use a different local port, update all three processes consistently:

```sh
# Dashboard service
./adviser/run.sh --port 9000

# Observed client, in a separate terminal
AMEISE_OBSERVER_URL=http://127.0.0.1:9000/api/v2/events ./launch_observed_client.sh

# Optional adviser, with OPENAI_API_KEY and OPENAI_MODEL set
./adviser/run-agent.sh --url http://127.0.0.1:9000
```

Then open **http://127.0.0.1:9000**. The service accepts only loopback bind addresses.

Use `./adviser/run.sh --data-dir /path/to/data` to change the storage directory. Set `AMEISE_OBSERVER_ENABLED=false` when launching the observed client to disable event publication. For further service and runner options:

```sh
./adviser/run.sh --help
./adviser/run-agent.sh --help
```

## Checks and further documentation

Run the Rust tests with:

```sh
cargo test --locked --manifest-path adviser/Cargo.toml
```

With JDK 8 and the external client distribution available, also run:

```sh
./adviser/client/test.sh
```

See the [integration guide](docs/json-api-integration.md) for the API, observation format, custom model adapters, and live validation steps. The [simulation guide](docs/README.md) explains the project rules, and the [adviser guidelines](adviser/AGENT_GUIDELINES.md) describe how advice is produced. Some older documentation links refer to separately supplied PDFs or client sources that are not tracked in this repository.
