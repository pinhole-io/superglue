//! Scheduler tests. Host steps do not call a provider.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use secrecy::SecretString;
use serde_json::{Value, json};
use superglue::Client;
use superglue::cancel::CancellationToken;
use superglue::chat::ChatOptions;
use superglue::graph::{Graph, GraphCall, GraphError, GraphSpec};
use superglue::guardrails::GuardrailRegistry;
use superglue::hooks::HookRegistry;
use superglue::http::{ClientConfig, HttpClient};
use superglue::tools::{Tool, ToolRegistry, ToolSpec};

fn client() -> Client {
    Client::builder().api_key("test").build().expect("client")
}

struct Slow {
    current: Arc<AtomicUsize>,
    max_in_flight: Arc<AtomicUsize>,
    label: &'static str,
}

#[async_trait]
impl GraphCall for Slow {
    async fn call(&self, _client: &Client, _state: &Value) -> Result<Value, GraphError> {
        let now = self.current.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_in_flight.fetch_max(now, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(40)).await;
        self.current.fetch_sub(1, Ordering::SeqCst);
        Ok(Value::String(self.label.into()))
    }
}

#[tokio::test]
async fn parallel_nodes_join_on_both_keys() {
    let current = Arc::new(AtomicUsize::new(0));
    let max_in_flight = Arc::new(AtomicUsize::new(0));
    let graph = Graph::builder("join")
        .call_with(
            "a",
            Arc::new(Slow {
                current: Arc::clone(&current),
                max_in_flight: Arc::clone(&max_in_flight),
                label: "A",
            }),
        )
        .call_with(
            "b",
            Arc::new(Slow {
                current,
                max_in_flight: Arc::clone(&max_in_flight),
                label: "B",
            }),
        )
        .call("join", |_client, state| {
            let a = state["a"].as_str().unwrap_or("");
            let b = state["b"].as_str().unwrap_or("");
            Ok(Value::String(format!("{a}+{b}")))
        })
        .depends("join", ["a", "b"])
        .output("join")
        .build()
        .unwrap();

    let answer = client().run_graph(&graph, json!({})).await.unwrap();
    assert_eq!(answer, json!("A+B"));
    assert!(max_in_flight.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn when_selects_the_matching_branch() {
    let skipped = Arc::new(AtomicUsize::new(0));
    let skipped_step = Arc::clone(&skipped);
    let graph = Graph::builder("route")
        .call("grade", |_client, _state| Ok(json!({"pass": true})))
        .call("yes", |_client, _state| Ok(json!("yes")))
        .call("no", move |_client, _state| {
            skipped_step.fetch_add(1, Ordering::SeqCst);
            Ok(json!("no"))
        })
        .when("grade", "pass", json!(true), "yes")
        .when("grade", "pass", json!(false), "no")
        .output("yes")
        .build()
        .unwrap();

    let answer = client().run_graph(&graph, json!({})).await.unwrap();
    assert_eq!(answer, json!("yes"));
    assert_eq!(skipped.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn cycle_with_one_visit_fails_at_build() {
    let error = Graph::builder("cycle")
        .call("again", |_client, _state| Ok(json!("x")))
        .when_fn("again", "again", |_| true)
        .output("again")
        .build()
        .unwrap_err();
    assert!(matches!(error, GraphError::UnboundCycle));
}

#[tokio::test]
async fn loop_stops_at_visit_limit() {
    let graph = Graph::builder("loop")
        .call("start", |_client, _state| Ok(json!("go")))
        .call("again", |_client, _state| Ok(json!("x")))
        .when_fn("start", "again", |_| true)
        .when_fn("again", "again", |_| true)
        .visits("again", 2)
        .output("again")
        .build()
        .unwrap();
    let error = client().run_graph(&graph, json!({})).await.unwrap_err();
    assert!(matches!(error, GraphError::VisitLimit(id) if id == "again"));
}

#[tokio::test]
async fn same_wave_writers_conflict_at_runtime() {
    let graph = Graph::builder("conflict")
        .call("left", |_client, _state| Ok(json!(1)))
        .call("right", |_client, _state| Ok(json!(2)))
        .call("x", |_client, _state| Ok(json!("x")))
        .call("y", |_client, _state| Ok(json!("y")))
        .when_fn("left", "x", |_| true)
        .when_fn("right", "y", |_| true)
        .write("x", "out")
        .write("y", "out")
        .output("out")
        .build()
        .unwrap();
    let error = client().run_graph(&graph, json!({})).await.unwrap_err();
    assert!(matches!(error, GraphError::StateConflict(key) if key == "out"));
}

#[test]
fn static_wave_rejects_shared_write_key() {
    let error = Graph::builder("static")
        .call("a", |_client, _state| Ok(json!("a")))
        .call("b", |_client, _state| Ok(json!("b")))
        .write("a", "out")
        .write("b", "out")
        .output("out")
        .build()
        .unwrap_err();
    assert!(matches!(error, GraphError::StateConflict(key) if key == "out"));
}

#[tokio::test]
async fn cancelled_token_stops_before_the_wave() {
    let token = CancellationToken::new();
    token.cancel();
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_step = Arc::clone(&hits);
    let graph = Graph::builder("cancel")
        .call("a", move |_client, _state| {
            hits_step.fetch_add(1, Ordering::SeqCst);
            Ok(json!("a"))
        })
        .output("a")
        .build()
        .unwrap();
    let options = ChatOptions {
        api_key: SecretString::from("test".to_string()),
        base_url: "http://127.0.0.1:9".into(),
        model: "mock".into(),
        cancel: Some(token),
        ..ChatOptions::default()
    };
    let http = Arc::new(HttpClient::new(ClientConfig::default()).unwrap());
    let client = Client::from_parts(
        options,
        Arc::new(ToolRegistry::new()),
        Arc::new(HookRegistry::new()),
        Arc::new(GuardrailRegistry::new()),
        http,
    );
    let error = client.run_graph(&graph, json!({})).await.unwrap_err();
    assert!(matches!(error, GraphError::Cancelled));
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn missing_input_fails_before_a_node() {
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_step = Arc::clone(&hits);
    let graph = Graph::builder("input")
        .require("question")
        .call("a", move |_client, _state| {
            hits_step.fetch_add(1, Ordering::SeqCst);
            Ok(json!("a"))
        })
        .output("a")
        .build()
        .unwrap();
    let error = client().run_graph(&graph, json!({})).await.unwrap_err();
    assert!(matches!(error, GraphError::MissingInput(key) if key == "question"));
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn tool_step_reads_args_from_state() {
    struct Echo;
    #[async_trait]
    impl Tool for Echo {
        fn spec(&self) -> ToolSpec {
            ToolSpec::new("echo", json!({"type": "object"}))
        }
        async fn call(&self, arguments: Value) -> Result<Value, superglue::tools::ToolInvokeError> {
            Ok(arguments)
        }
    }
    let registry = ToolRegistry::new();
    registry.register(Arc::new(Echo)).await.unwrap();
    let options = ChatOptions {
        api_key: SecretString::from("test".to_string()),
        base_url: "http://127.0.0.1:9".into(),
        model: "mock".into(),
        ..ChatOptions::default()
    };
    let client = Client::from_parts(
        options,
        Arc::new(registry),
        Arc::new(HookRegistry::new()),
        Arc::new(GuardrailRegistry::new()),
        Arc::new(HttpClient::new(ClientConfig::default()).unwrap()),
    );
    let graph = Graph::builder("tool")
        .call("args", |_client, state| {
            Ok(json!({"city": state["question"].as_str().unwrap_or("")}))
        })
        .tool("lookup", "echo", "args")
        .depends("lookup", ["args"])
        .require("question")
        .output("lookup")
        .build()
        .unwrap();
    let answer = client
        .run_graph(&graph, json!({"question": "Paris"}))
        .await
        .unwrap();
    assert_eq!(answer, json!({"city": "Paris"}));
}

#[test]
fn spec_round_trip_and_unknown_kind() {
    let graph = Graph::builder("spec")
        .require("question")
        .map("draft", "About {question}")
        .output("draft")
        .build()
        .unwrap();
    let spec = graph.to_spec().unwrap();
    let encoded = spec.to_json().unwrap();
    let loaded = Graph::from_spec(GraphSpec::from_json(&encoded).unwrap()).unwrap();
    assert_eq!(loaded.name(), "spec");

    let error = GraphSpec::from_json(
        r#"{"name":"x","output":"a","nodes":[{"id":"a","step":{"kind":"shell","prompt":"hi"}}]}"#,
    )
    .unwrap_err();
    assert!(matches!(error, GraphError::UnknownStep));
}

#[test]
fn multi_judge_panel_builds() {
    use superglue::agents::AgentSpec;
    use superglue::judge::JudgeBuilder;

    let writer = AgentSpec::new("desk", "a careful support agent");
    let safety = JudgeBuilder::new("desk.safety")
        .noul("safe", "The reply invents nothing")
        .done()
        .noul_output("pass", "safe")
        .threshold(0.5)
        .done()
        .build()
        .unwrap();
    let tone = JudgeBuilder::new("desk.tone")
        .choice("tone", "Which tone does this reply use?")
        .option("calm", "Polite")
        .option("terse", "Short")
        .option("rude", "Blames the customer")
        .done()
        .choice_output("label", "tone")
        .map_text("calm", "calm")
        .map_text("terse", "terse")
        .map_text("rude", "rude")
        .emit_confidence()
        .done()
        .build()
        .unwrap();
    let coverage = JudgeBuilder::new("desk.coverage")
        .noul("answers", "The reply addresses the request")
        .done()
        .noul_output("pass", "answers")
        .threshold(0.5)
        .done()
        .build()
        .unwrap();
    let graph = Graph::builder("desk")
        .require("ticket")
        .agent("draft", writer.clone(), "Reply to {ticket}")
        .judge("safety", safety, "draft")
        .judge("tone", tone, "draft")
        .judge("coverage", coverage, "draft")
        .map("hold", "We need a moment to check this ticket.")
        .agent("reply", writer, "Use {draft} {safety} {tone} {coverage}")
        .depends("safety", ["draft"])
        .depends("tone", ["draft"])
        .depends("coverage", ["draft"])
        .depends("reply", ["tone", "coverage"])
        .when("safety", "pass", json!(true), "reply")
        .otherwise("safety", "hold")
        .write("hold", "answer")
        .write("reply", "answer")
        .output("answer")
        .build()
        .unwrap();
    let spec = graph.to_spec().unwrap();
    let safety_node = spec.nodes.iter().find(|node| node.id == "safety").unwrap();
    assert_eq!(safety_node.when[0].target, "reply");
    assert_eq!(safety_node.otherwise.as_deref(), Some("hold"));
    let reply = spec.nodes.iter().find(|node| node.id == "reply").unwrap();
    assert_eq!(
        reply.depends,
        vec!["tone".to_string(), "coverage".to_string()]
    );
    assert_eq!(reply.write.as_deref(), Some("answer"));
    assert!(
        spec.nodes
            .iter()
            .any(|node| node.id == "tone" && node.depends == vec!["draft".to_string()])
    );
    Graph::from_spec(spec).unwrap();
}

#[test]
fn closure_graph_is_not_serializable() {
    let graph = Graph::builder("host")
        .call("a", |_client, _state| Ok(json!("a")))
        .output("a")
        .build()
        .unwrap();
    assert!(matches!(
        graph.to_spec().unwrap_err(),
        GraphError::NotSerializable
    ));
}
