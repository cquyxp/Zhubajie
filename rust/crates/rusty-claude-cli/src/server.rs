use std::env;

use runtime::{McpServer, McpServerSpec, McpTool, WorkerRegistry};
use tools::{execute_tool, mvp_tool_specs};

use crate::CliOutputFormat;

pub(crate) fn run_server(
    port: u16,
    output_format: CliOutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let registry = WorkerRegistry::new();

    match output_format {
        CliOutputFormat::Text => println!("Starting server on port {port}..."),
        CliOutputFormat::Json => println!(
            "{}",
            serde_json::json!({
                "type": "server_start",
                "port": port
            })
        ),
    }

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async { runtime::api_server::start_server(registry, port).await })?;

    Ok(())
}

/// the same surface the in-process agent loop uses.
pub(crate) fn run_mcp_serve() -> Result<(), Box<dyn std::error::Error>> {
    let tools = mvp_tool_specs()
        .into_iter()
        .map(|spec| McpTool {
            name: spec.name.to_string(),
            description: Some(spec.description.to_string()),
            input_schema: Some(spec.input_schema),
            annotations: None,
            meta: None,
        })
        .collect();

    let spec = McpServerSpec {
        server_name: "claw".to_string(),
        server_version: crate::VERSION.to_string(),
        tools,
        tool_handler: Box::new(execute_tool),
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let mut server = McpServer::new(spec);
        server.run().await
    })?;
    Ok(())
}

/// Read `.claw/worker-state.json` from the current working directory and print it.
pub(crate) fn run_worker_state(
    output_format: CliOutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = env::current_dir()?;
    let state_path = cwd.join(".claw").join("worker-state.json");
    if !state_path.exists() {
        return Err(format!(
            "no worker state file found at {} — run a worker first",
            state_path.display()
        )
        .into());
    }
    let raw = std::fs::read_to_string(&state_path)?;
    match output_format {
        CliOutputFormat::Text => println!("{raw}"),
        CliOutputFormat::Json => {
            let _: serde_json::Value = serde_json::from_str(&raw)?;
            println!("{raw}");
        }
    }
    Ok(())
}
