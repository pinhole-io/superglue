package com.superglue.kt.examples

import com.superglue.kt.AgentRunSpec
import com.superglue.kt.BatchRequestItem
import com.superglue.kt.BlocklistAction
import com.superglue.kt.ClientConfig
import com.superglue.kt.GuardSide
import com.superglue.kt.GuardrailCallback
import com.superglue.kt.HookErrorStrategy
import com.superglue.kt.HookStage
import com.superglue.kt.JsonCallback
import com.superglue.kt.SuperglueClient
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

/**
 * JUnit counterparts for examples 17–21, 28, 32, 34–37, 43, and the short demo.
 * Live sections need `OPENAI_API_KEY`, except where a test names another variable.
 */
class LaterExamplesTest {
    @Test
    fun example17_batch_completions() {
        runLive(17) {
            openExampleClient().use { client ->
                val outcome =
                    client.batch(
                        listOf(
                            BatchRequestItem(id = "one", prompt = "Say one."),
                            BatchRequestItem(id = "two", prompt = "Say two."),
                        ),
                        maxConcurrent = 2,
                    )
                assertEquals(2, outcome.totalRequests)
                assertEquals(2, outcome.results.size)
                assertTrue(outcome.successful >= 1)
            }
        }
    }

    @Test
    fun example18_hooks() {
        runLive(18) {
            val calls = AtomicInteger(0)
            openExampleClient().use { client ->
                client.registerHook(
                    HookStage.PRE_COMPLETION,
                    "observe",
                    HookErrorStrategy.SKIP,
                    JsonCallback { _ ->
                        calls.incrementAndGet()
                        ""
                    },
                )
                val result = client.complete("Say hello in one word.")
                assertHasContent(result.content)
                assertTrue(calls.get() >= 1)
            }
        }
    }

    @Test
    fun example19_guardrails() {
        runLive(19) {
            openExampleClient().use { client ->
                client.addBlocklistGuardrail(
                    listOf("profanity", "\\bbadword\\b"),
                    BlocklistAction.BLOCK,
                    GuardSide.INPUT,
                    "profanity-filter",
                )
                var blocked = false
                try {
                    client.complete("Please say the word badword.")
                } catch (_: Exception) {
                    blocked = true
                }
                assertTrue(blocked)
            }

            openExampleClient().use { client ->
                client.registerGuardrail(
                    GuardrailCallback { _, content ->
                        if (content.contains("competitor_a", ignoreCase = true)) {
                            throw IllegalArgumentException("competitor mention detected")
                        }
                        ""
                    },
                    GuardSide.INPUT,
                    "competitor-filter",
                )
                var blocked = false
                try {
                    client.complete("Tell me about competitor_a.")
                } catch (_: Exception) {
                    blocked = true
                }
                assertTrue(blocked)

                val allowed = client.complete("Name one fact about the Moon. One sentence.")
                assertHasContent(allowed.content)
            }
        }
    }

    @Test
    fun example20_agents() {
        runLive(20) {
            openExampleClient().use { client ->
                val spec =
                    AgentRunSpec(
                        name = "Helper",
                        persona = "a concise assistant",
                        goals = listOf("Answer in one sentence"),
                    )
                val result = client.runAgent(spec, "What is 2 + 2?")
                assertHasContent(result.content)
            }
        }
    }

    @Test
    fun example21_executors() {
        val note =
            "SimpleExecutor, AgentExecutor, and AgentStructuredExecutor exist in Python. " +
                "The Kotlin binding does not expose them. " +
                "Use SuperglueClient, SuperglueAgentEngine, hooks, and guardrails."
        assertTrue(note.contains("SimpleExecutor"))
        assertTrue(note.contains("AgentExecutor"))
        assertTrue(note.contains("AgentStructuredExecutor"))
        assertTrue(note.contains("Python"))
        println(note)
    }

    @Test
    fun example28_mcp_tools() {
        if (System.getenv("MCP_RUN") != "1") {
            println("Skip MCP example (set MCP_RUN=1 and ensure npx is available)")
            return
        }
        runLive(28) {
            openExampleClient().use { client ->
                client.connectMcpStdio(
                    "npx",
                    """["-y","@modelcontextprotocol/server-everything"]""",
                    prefix = "mcp",
                )
                val result =
                    client.complete(
                        "Name one available tool with the mcp__ prefix. One line only.",
                        reasoningEffort = "none",
                    )
                assertHasContent(result.content)
            }
        }
    }

    @Test
    fun example32_context_optimization() {
        val config =
            ClientConfig(
                apiKey = "k",
                toolMode = "dynamic",
                condenseToolMessages = true,
            )
        assertEquals("dynamic", config.toolMode)
        assertTrue(config.condenseToolMessages)
        runLive(32) {
            openExampleClient {
                it.copy(
                    systemPrompt = "You are a helpful assistant. Use tools when needed.",
                    toolMode = "dynamic",
                    condenseToolMessages = true,
                )
            }.use { client ->
                client.registerContextTools()
                val result = client.complete(
                    "What's the weather in Paris?",
                    reasoningEffort = "none",
                )
                assertHasContent(result.content)
                assertTrue(result.rounds >= 1)
            }
        }
    }

    @Test
    fun example34_context_optimization_live() {
        runLive(34) {
            val prompt = "What's the weather in Paris?"
            val standard =
                openExampleClient {
                    it.copy(systemPrompt = "You are a helpful assistant. Use tools when needed.")
                }
            val optimized =
                openExampleClient {
                    it.copy(
                        systemPrompt = "You are a helpful assistant. Use tools when needed.",
                        toolMode = "dynamic",
                        condenseToolMessages = true,
                    )
                }
            try {
                standard.registerContextTools()
                optimized.registerContextTools()
                val plain = standard.complete(prompt, reasoningEffort = "none")
                val condensed = optimized.complete(prompt, reasoningEffort = "none")
                assertHasContent(plain.content)
                assertHasContent(condensed.content)
                println("standard rounds=${plain.rounds} usage=${plain.usage}")
                println("optimized rounds=${condensed.rounds} usage=${condensed.usage}")
            } finally {
                standard.close()
                optimized.close()
            }
        }
    }

    @Test
    fun example35_code_tool_mode() {
        val config = ClientConfig(apiKey = "k", toolMode = "code")
        assertEquals("code", config.toolMode)
        runLive(35) {
            openExampleClient {
                it.copy(
                    toolMode = "code",
                    systemPrompt =
                        "You are a helpful assistant. Call request_tools first, then use the code " +
                            "tool to invoke matched tools from JavaScript (tools.<name>(args)).",
                )
            }.use { client ->
                client.registerContextTools()
                val result = client.complete(
                    "What's the weather in Paris? Return only the temperature.",
                    reasoningEffort = "none",
                )
                assertHasContent(result.content)
            }
        }
    }

    @Test
    fun example37_systemone() {
        val key = System.getenv("TYPESAFE_API_KEY").orEmpty()
        if (key.isEmpty()) {
            println("Set TYPESAFE_API_KEY to run example 37.")
            return
        }
        val state =
            Json.encodeToString(
                String.serializer(),
                "Hi, I've been trying to connect my Stripe account for 3 days and it keeps failing. " +
                    "I'm losing sales. Please help ASAP.",
            )
        val questions =
            """
            {
              "is_urgent": {
                "type": "noul",
                "instructions": "The message conveys urgency or time-sensitivity"
              },
              "department": {
                "type": "choice",
                "instructions": "Which team should handle this",
                "criteria": {
                  "billing": "Payment or subscription issues",
                  "technical": "Bugs or integration problems",
                  "sales": "Pricing or account questions"
                }
              },
              "frustration": {
                "type": "score",
                "instructions": "How frustrated the customer appears",
                "criteria": [
                  "Calm, just stating facts",
                  "Frustrated but civil",
                  "Very angry, strong language"
                ]
              }
            }
            """.trimIndent()
        SuperglueClient
            .open(
                ClientConfig(
                    apiKey = "unused",
                    apiKeys = mapOf("typesafe" to key),
                ),
            ).use { client ->
                val raw = client.systemOne(state, questions)
                val answers = Json.parseToJsonElement(raw).jsonObject.getValue("answers").jsonObject
                assertTrue(answers.containsKey("is_urgent"))
                assertTrue(answers.containsKey("department"))
                assertTrue(answers.containsKey("frustration"))
                println("department: ${answers["department"]}")
                println("frustration: ${answers["frustration"]}")
                println("is_urgent: ${answers["is_urgent"]}")
            }
    }

    /** Realtime voice with one tool call. The assistant transcript arrives as deltas. */
    @Test
    fun example36_realtime_voice() {
        runLive(36) {
            val model = System.getenv("OPENAI_REALTIME_MODEL") ?: "gpt-realtime"
            val config =
                """
                {
                  "provider": "openai",
                  "model": "$model",
                  "instructions": "Answer in one short sentence.",
                  "tools": [{
                    "name": "lookup_order",
                    "description": "Return the status of an order by id.",
                    "parameters": {
                      "type": "object",
                      "properties": {"order_id": {"type": "string"}},
                      "required": ["order_id"]
                    }
                  }]
                }
                """.trimIndent()
            // The callback runs on a native thread. Hand each event to the test thread.
            val events = LinkedBlockingQueue<Pair<JsonObject, ByteArray?>>()
            openExampleClient().use { client ->
                client.voiceConnect(config) { eventJson, audio ->
                    events.put(Json.parseToJsonElement(eventJson).jsonObject to audio)
                }.use { session ->
                    session.sendText("Use lookup_order for order 42 and tell me its status.")
                    var audioBytes = 0
                    val reply = StringBuilder()
                    var toolCalls = 0
                    var waitingForToolReply = false
                    while (true) {
                        val (event, audio) =
                            events.poll(60, TimeUnit.SECONDS)
                                ?: error("no voice event for 60 seconds")
                        when (event.getValue("type").jsonPrimitive.content) {
                            "output_audio" -> audioBytes += audio?.size ?: 0
                            "output_transcript" -> reply.append(event.getValue("text").jsonPrimitive.content)
                            "function_call" -> {
                                val callId = event.getValue("call_id").jsonPrimitive.content
                                val name = event.getValue("name").jsonPrimitive.content
                                println("tool call: $name(${event.getValue("arguments").jsonPrimitive.content})")
                                toolCalls += 1
                                waitingForToolReply = true
                                // This also asks the model for the next response.
                                session.submitToolResult(callId, """{"order_id":"42","status":"shipped"}""")
                            }
                            "response_done" -> {
                                if (!waitingForToolReply) break
                                waitingForToolReply = false
                            }
                            "error" -> error("voice error: ${event["message"]}")
                            "closed" -> break
                        }
                    }
                    println("assistant: $reply")
                    println("audio: $audioBytes bytes of PCM16")
                    assertTrue(audioBytes > 0 || reply.isNotEmpty())
                    assertTrue(toolCalls <= 1)
                }
            }
        }
    }

    @Test
    fun example43_graph() {
        runLive(43) {
            val model = exampleModel()
            val typesafe = System.getenv("TYPESAFE_API_KEY").orEmpty()
            val spec =
                if (typesafe.isEmpty()) {
                    println("TYPESAFE_API_KEY is unset. The graph drafts, then polishes.")
                    plainGraph(model)
                } else {
                    println("The graph drafts, judges, then polishes or keeps the draft.")
                    judgedGraph(model)
                }
            val baseUrl = baseUrlFor(exampleApiKey(), System.getenv("OPENAI_BASE_URL"))
            openExampleClient {
                val config = it.copy(baseUrl = baseUrl)
                if (typesafe.isEmpty()) {
                    config
                } else {
                    config.copy(apiKeys = mapOf("typesafe" to typesafe))
                }
            }.use { client ->
                println("Question: Rust ownership")
                println("Running the graph...")
                val answer = client.runGraph(spec, """{"question":"Rust ownership"}""")
                println("Answer: $answer")
                assertTrue(answer.isNotBlank() && answer != "null")
            }
        }
    }

    @Test
    fun example_demo() {
        if (exampleApiKey().isEmpty()) {
            println("Set OPENAI_API_KEY to run the demo live.")
            return
        }
        openExampleClient {
            it.copy(systemPrompt = "You are a concise assistant.")
        }.use { client ->
            client.registerTool(
                "get_weather",
                "Get weather for a city.",
                """{"type":"object","properties":{"location":{"type":"string"}},"required":["location"]}""",
                JsonCallback { _ ->
                    """{"location":"Paris","temperature":18,"condition":"cloudy"}"""
                },
            )
            val result = client.complete(
                "What's the weather in Paris? One sentence.",
                reasoningEffort = "none",
            )
            assertHasContent(result.content)
        }
    }

    private fun writerJson(model: String): String {
        val encoded = Json.encodeToString(String.serializer(), model)
        return """
            {
              "name": "writer",
              "persona": "a careful writer",
              "goals": [],
              "instructions": [],
              "constraints": [],
              "model": $encoded,
              "max_tool_rounds": 16,
              "max_output_retries": 3,
              "system_prompt_override": null,
              "reasoning_effort": null
            }
            """.trimIndent()
    }

    private fun draftNode(model: String): String =
        """
        {
          "id": "draft",
          "visits": 1,
          "step": {
            "kind": "agent",
            "spec": ${writerJson(model)},
            "prompt": "Write a short draft about {question}. Use at most 4 sentences."
          }
        }
        """.trimIndent()

    private fun polishNode(model: String, write: String?): String {
        val extra = if (write == null) """"depends": ["draft"]""" else """"write": "$write""""
        return """
            {
              "id": "polish",
              "visits": 1,
              $extra,
              "step": {
                "kind": "agent",
                "spec": ${writerJson(model)},
                "prompt": "Polish this draft. Keep it under 4 sentences.\n{draft}"
              }
            }
            """.trimIndent()
    }

    private fun plainGraph(model: String): String =
        """
        {
          "name": "review",
          "require": ["question"],
          "output": "polish",
          "nodes": [${draftNode(model)}, ${polishNode(model, null)}]
        }
        """.trimIndent()

    private fun judgedGraph(model: String): String =
        """
        {
          "name": "review",
          "require": ["question"],
          "output": "answer",
          "nodes": [
            ${draftNode(model)},
            {
              "id": "grade",
              "visits": 1,
              "depends": ["draft"],
              "step": {
                "kind": "judge",
                "input": "draft",
                "config": {
                  "id": "review.pass",
                  "model": "typesafe:jev-latest",
                  "timeout_ms": 400,
                  "on_error": "error",
                  "questions": {
                    "good": {
                      "type": "noul",
                      "instructions": "The draft answers the question in clear prose"
                    }
                  },
                  "outputs": [
                    {
                      "kind": "noul",
                      "name": "pass",
                      "from": "good",
                      "threshold": 0.5,
                      "required": false
                    }
                  ]
                }
              },
              "when": [{"path": "pass", "equals": true, "target": "polish"}],
              "otherwise": "keep"
            },
            ${polishNode(model, "answer")},
            {
              "id": "keep",
              "write": "answer",
              "visits": 1,
              "step": {"kind": "map", "template": "{draft}"}
            }
          ]
        }
        """.trimIndent()
}
