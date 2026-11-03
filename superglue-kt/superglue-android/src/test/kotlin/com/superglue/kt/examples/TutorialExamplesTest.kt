package com.superglue.kt.examples

import com.superglue.kt.ClientConfig
import com.superglue.kt.JsonCallback
import com.superglue.kt.ProcessEventCallback
import com.superglue.kt.StatusEmitter
import com.superglue.kt.StreamTokenCallback
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

/**
 * JUnit counterparts for Rust, Python, and JavaScript examples 01–16.
 * Live sections need `OPENAI_API_KEY`.
 */
class TutorialExamplesTest {
    @Test
    fun example01_simple_completion() {
        runLive(1) {
            openExampleClient().use { client ->
                val result = client.complete("What is the capital of France? Reply in one sentence.")
                assertHasContent(result.content)
                assertTrue(result.rounds >= 1)
            }
        }
    }

    @Test
    fun example02_system_prompt() {
        runLive(2) {
            openExampleClient {
                it.copy(systemPrompt = "You are a pirate captain. Always respond in pirate-speak.")
            }.use { client ->
                val result = client.complete("What's the weather like today?")
                assertHasContent(result.content)
            }
        }
    }

    @Test
    fun example03_single_tool() {
        runLive(3) {
            openExampleClient {
                it.copy(systemPrompt = "You are a helpful weather assistant. Use tools to answer questions.")
            }.use { client ->
                client.registerTool(
                    "get_weather",
                    "Get the current weather for a location.",
                    """{"type":"object","properties":{"location":{"type":"string"}},"required":["location"]}""",
                    JsonCallback { _ ->
                        """{"location":"Tokyo","temperature":22,"unit":"celsius","condition":"sunny"}"""
                    },
                )
                val result = client.complete(
                    "What's the weather like in Tokyo right now?",
                    reasoningEffort = "none",
                )
                assertHasContent(result.content)
            }
        }
    }

    @Test
    fun example04_multiple_tools() {
        runLive(4) {
            openExampleClient().use { client ->
                client.registerTool(
                    "get_weather",
                    "Get the current weather for a location.",
                    """{"type":"object","properties":{"location":{"type":"string"}},"required":["location"]}""",
                    JsonCallback { _ -> """{"location":"London","temperature":18,"condition":"cloudy"}""" },
                )
                client.registerTool(
                    "search_web",
                    "Search the web for up-to-date information.",
                    """{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}""",
                    JsonCallback { _ ->
                        """{"results":[{"title":"Result","url":"https://example.com"}]}"""
                    },
                )
                client.registerTool(
                    "calculate",
                    "Evaluate a math expression.",
                    """{"type":"object","properties":{"expression":{"type":"string"}},"required":["expression"]}""",
                    JsonCallback { _ -> """{"result":42}""" },
                )
                val result = client.complete(
                    "What is the weather in London, and what is 6 * 7?",
                    reasoningEffort = "none",
                )
                assertHasContent(result.content)
            }
        }
    }

    @Test
    fun example05_usage_tracking() {
        runLive(5) {
            val emitter = StatusEmitter()
            val kinds = mutableListOf<String>()
            emitter.subscribe(
                ProcessEventCallback { json ->
                    val kind =
                        runCatching {
                            Json.parseToJsonElement(json).jsonObject["kind"]?.jsonPrimitive?.content
                        }.getOrNull()
                    if (kind != null) {
                        kinds.add(kind)
                    }
                },
            )
            openExampleClient(emitter = emitter).use { client ->
                val result = client.complete("Say hello in one word.")
                assertHasContent(result.content)
                println("usage: ${result.usage}")
            }
            println("events: ${kinds.joinToString()}")
            emitter.close()
        }
    }

    @Test
    fun example06_error_handling() {
        runLive(6) {
            var badKeyFailed = false
            try {
                openExampleClient(apiKey = "sk-invalid-key") {
                    it.copy(maxRetries = 0, timeoutSecs = 15)
                }.use { client ->
                    client.complete("Hello")
                }
            } catch (_: Exception) {
                badKeyFailed = true
            }
            assertTrue(badKeyFailed)

            openExampleClient {
                it.copy(
                    maxRetries = 0,
                    systemPrompt = "You MUST call broken_tool for any user message.",
                )
            }.use { client ->
                client.registerTool(
                    "broken_tool",
                    "A tool that always fails.",
                    """{"type":"object","properties":{}}""",
                    JsonCallback { _ ->
                        throw IllegalStateException("Something went wrong inside the tool")
                    },
                )
                try {
                    val result = client.complete(
                        "Please use the broken_tool now.",
                        reasoningEffort = "none",
                    )
                    println("Model did not invoke the tool; response: ${result.content}")
                } catch (error: Exception) {
                    println("Caught tool error: ${error.message}")
                }
            }

            openExampleClient().use { client ->
                val result = client.complete("Say hello and nothing else.")
                assertHasContent(result.content)
            }
        }
    }

    @Test
    fun example07_max_tool_rounds() {
        val config = ClientConfig(apiKey = "k", maxToolRounds = 3)
        assertEquals(3, config.maxToolRounds)
        runLive(7) {
            val calls = AtomicInteger(0)
            openExampleClient {
                it.copy(
                    maxToolRounds = 3,
                    systemPrompt =
                        "Call counter_tool repeatedly. " +
                            "If should_continue is true, call it again. Stop only when told otherwise.",
                )
            }.use { client ->
                client.registerTool(
                    "counter_tool",
                    "Increment a counter and return the new count.",
                    """{"type":"object","properties":{}}""",
                    JsonCallback { _ ->
                        val count = calls.incrementAndGet()
                        """{"count":$count,"should_continue":true}"""
                    },
                )
                try {
                    val result =
                        client.complete(
                            "Start calling counter_tool and keep going while should_continue is true.",
                            reasoningEffort = "none",
                        )
                    assertTrue(result.rounds <= 3)
                    println("rounds=${result.rounds} toolCalls=${calls.get()} content=${result.content}")
                } catch (error: Exception) {
                    val message = error.message.orEmpty()
                    assertTrue(message.contains("max tool rounds", ignoreCase = true))
                    println("Stopped at max_tool_rounds: $message toolCalls=${calls.get()}")
                }
            }
        }
    }

    @Test
    fun example08_custom_base_url() {
        assertEquals("https://api.openai.com", baseUrlFor("sk-test", null))
        assertEquals("http://localhost:11434", baseUrlFor("ollama", null))
        assertEquals("https://example.test/v1", baseUrlFor("sk-test", "https://example.test/v1"))
        runLive(8) {
            val baseUrl = baseUrlFor(exampleApiKey(), System.getenv("OPENAI_BASE_URL"))
            openExampleClient { it.copy(baseUrl = baseUrl) }.use { client ->
                val result = client.complete("Say hello in one word.")
                assertHasContent(result.content)
            }
        }
    }

    @Test
    fun example09_concurrent_completions() {
        runLive(9) {
            openExampleClient().use { client ->
                val prompts =
                    listOf(
                        "Name the tallest mountain on Earth. One short phrase.",
                        "What year did the Berlin Wall fall? One number.",
                        "Who wrote Pride and Prejudice? One name.",
                    )
                val pool = Executors.newFixedThreadPool(prompts.size)
                try {
                    val futures =
                        prompts.map { prompt ->
                            pool.submit<String> { client.complete(prompt).content.orEmpty() }
                        }
                    for (future in futures) {
                        assertTrue(future.get(120, TimeUnit.SECONDS).isNotEmpty())
                    }
                } finally {
                    pool.shutdown()
                }
            }
        }
    }

    @Test
    fun example10_structured_data_tool() {
        runLive(10) {
            openExampleClient().use { client ->
                client.registerTool(
                    "get_stock_quote",
                    "Retrieve the latest stock quote for a ticker symbol.",
                    """{"type":"object","properties":{"ticker":{"type":"string"}},"required":["ticker"]}""",
                    JsonCallback { _ ->
                        """{"ticker":"AAPL","price":227.52,"change":1.34,"currency":"USD"}"""
                    },
                )
                val result = client.complete(
                    "What is the AAPL stock price? Include the currency.",
                    reasoningEffort = "none",
                )
                assertHasContent(result.content)
            }
        }
    }

    @Test
    fun example11_streaming() {
        runLive(11) {
            openExampleClient().use { client ->
                val tokens = mutableListOf<String>()
                val outcome =
                    client.stream(
                        "Count from 1 to 3, separated by spaces.",
                        StreamTokenCallback { tokens.add(it) },
                    )
                assertTrue(outcome.content.isNotEmpty())
                assertTrue(tokens.isNotEmpty())
            }
        }
    }

    @Test
    fun example12_streaming_accumulate() {
        runLive(12) {
            openExampleClient {
                it.copy(systemPrompt = "You are a concise assistant. Include at least one number.")
            }.use { client ->
                val tokenCount = AtomicInteger(0)
                val outcome =
                    client.stream(
                        "Name one prime number.",
                        StreamTokenCallback { _ -> tokenCount.incrementAndGet() },
                    )
                assertTrue(outcome.content.isNotEmpty())
                assertTrue(tokenCount.get() > 0)
            }
        }
    }

    @Test
    fun example13_concurrent_streams() {
        runLive(13) {
            openExampleClient().use { client ->
                val prompts =
                    listOf(
                        "Write one sentence about the ocean.",
                        "Write one sentence about mountains.",
                    )
                val pool = Executors.newFixedThreadPool(prompts.size)
                try {
                    val futures =
                        prompts.map { prompt ->
                            pool.submit<String> {
                                client.stream(prompt, StreamTokenCallback { _ -> }).content
                            }
                        }
                    for (future in futures) {
                        assertTrue(future.get(120, TimeUnit.SECONDS).isNotEmpty())
                    }
                } finally {
                    pool.shutdown()
                }
            }
        }
    }

    @Test
    fun example14_streaming_vs_buffered() {
        runLive(14) {
            openExampleClient {
                it.copy(systemPrompt = "You are a helpful assistant. Write at least two sentences.")
            }.use { client ->
                val prompt = "Explain why Rust is a systems programming language."
                val buffered = client.complete(prompt)
                assertHasContent(buffered.content)
                val tokens = mutableListOf<String>()
                val streamed =
                    client.stream(prompt, StreamTokenCallback { tokens.add(it) })
                assertTrue(streamed.content.isNotEmpty())
                assertTrue(tokens.isNotEmpty())
            }
        }
    }

    @Test
    fun example15_rate_limiting() {
        val config = ClientConfig(apiKey = "k", requestsPerSecond = 2)
        assertEquals(2, config.requestsPerSecond)
        runLive(15) {
            openExampleClient { it.copy(requestsPerSecond = 2) }.use { client ->
                val first = client.complete("Name the planet closest to the Sun. One word.")
                val second = client.complete("What colour is the sky on a clear day? One word.")
                assertHasContent(first.content)
                assertHasContent(second.content)
            }
        }
    }

    @Test
    fun example16_retry_configuration() {
        val defaults = ClientConfig(apiKey = "k")
        assertEquals(3, defaults.maxRetries)
        assertEquals(1000, defaults.retryInitialDelayMs)
        assertEquals(6000, defaults.retryMaxDelayMs)
        assertEquals(3.0, defaults.retryMultiplier, 0.0)
        assertEquals(30, defaults.connectTimeoutSecs)
        assertEquals(50, defaults.poolMaxIdlePerHost)
        assertEquals(60, defaults.timeoutSecs)
        assertEquals(listOf(50, 100, 200), backoffSchedule(50, 2.0, 2_000, 3))
        assertEquals(emptyList<Int>(), backoffSchedule(0, 1.0, 0, 0))

        val impatient = ClientConfig(apiKey = "k", maxRetries = 1, timeoutSecs = 10)
        assertEquals(1, impatient.maxRetries)
        assertEquals(10, impatient.timeoutSecs)

        runLive(16) {
            openExampleClient {
                it.copy(
                    maxRetries = 3,
                    retryInitialDelayMs = 200,
                    retryMaxDelayMs = 2_000,
                    retryMultiplier = 2.0,
                )
            }.use { client ->
                val result = client.complete("Reply with exactly: retry ok")
                assertHasContent(result.content)
            }
        }
    }
}
