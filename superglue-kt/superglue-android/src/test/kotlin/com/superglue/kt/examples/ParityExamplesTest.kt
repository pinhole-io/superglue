package com.superglue.kt.examples

import com.superglue.kt.ClientConfig
import com.superglue.kt.SuperglueClient
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * JUnit counterparts for examples 38 and 40–44.
 * Example 38 needs the provider key named in the test.
 * Examples 40 and 42 need `TYPESAFE_API_KEY`.
 * Example 41 needs `OPENAI_API_KEY`.
 */
class ParityExamplesTest {
    @Test
    fun example38_file_transcription() {
        val provider = System.getenv("SUPERGLUE_STT_PROVIDER") ?: "xai"
        val keyVar =
            when (provider) {
                "openai" -> "OPENAI_API_KEY"
                "groq" -> "GROQ_API_KEY"
                else -> "XAI_API_KEY"
            }
        if (System.getenv(keyVar).isNullOrEmpty()) {
            println("Skip file transcription example (set $keyVar)")
            return
        }
        val file = audioFile()
        assertTrue("audio file is missing: ${file.path}", file.isFile)
        SuperglueClient.open(ClientConfig(apiKey = "unused")).use { client ->
            val raw = client.transcribe(file.name, file.readBytes(), provider)
            val result = Json.parseToJsonElement(raw).jsonObject
            println("${result["provider"]} model ${result["model"]}")
            println(result["text"])
            assertTrue(result.containsKey("text"))
        }
    }

    @Test
    fun example40_judge() {
        val key = System.getenv("TYPESAFE_API_KEY").orEmpty()
        if (key.isEmpty()) {
            println("Set TYPESAFE_API_KEY to run example 40.")
            return
        }
        val config =
            """
            {
              "id": "example.urgency",
              "model": "typesafe:jev-latest",
              "timeout_ms": 400,
              "on_error": "error",
              "questions": {
                "is_urgent": {
                  "type": "noul",
                  "instructions": "The message conveys urgency or time-sensitivity"
                }
              },
              "outputs": [{
                "kind": "noul",
                "name": "urgent",
                "from": "is_urgent",
                "threshold": 0.5,
                "required": false
              }]
            }
            """.trimIndent()
        val state =
            "Hi, I've been trying to connect my Stripe account for 3 days and it keeps failing. " +
                "I'm losing sales. Please help ASAP."
        typesafeClient(key).use { client ->
            val raw = client.judge(config, Json.encodeToString(String.serializer(), state))
            val verdict = Json.parseToJsonElement(raw).jsonObject
            println("urgent: ${verdict["urgent"]}")
            assertTrue(verdict.containsKey("urgent"))
        }
    }

    @Test
    fun example41_structured_output() {
        runLive(41) {
            val schema =
                """
                {
                  "name": "person",
                  "description": "A person mentioned in the text.",
                  "schema": {
                    "type": "object",
                    "properties": {
                      "name": {"type": "string"},
                      "born": {"type": "integer"},
                      "occupation": {"type": "string"}
                    },
                    "required": ["name", "born", "occupation"],
                    "additionalProperties": false
                  }
                }
                """.trimIndent()
            val baseUrl = baseUrlFor(exampleApiKey(), System.getenv("OPENAI_BASE_URL"))
            openExampleClient { it.copy(baseUrl = baseUrl) }.use { client ->
                val raw = client.completeObject(
                    "Ada Lovelace, born in 1815, was an English mathematician.",
                    schema,
                )
                val person = Json.parseToJsonElement(raw).jsonObject
                println("name: ${person["name"]}")
                println("born: ${person["born"]}")
                println("occupation: ${person["occupation"]}")
                assertTrue(person.containsKey("name"))
                assertTrue(person.containsKey("born"))
                assertTrue(person.containsKey("occupation"))
            }
        }
    }

    @Test
    fun example42_judge_advanced() {
        val key = System.getenv("TYPESAFE_API_KEY").orEmpty()
        if (key.isEmpty()) {
            println("Set TYPESAFE_API_KEY to run example 42.")
            return
        }
        val config =
            """
            {
              "id": "example.triage",
              "model": "typesafe:jev-latest",
              "timeout_ms": 400,
              "state_chars": 240,
              "on_error": "error",
              "questions": {
                "department": {
                  "type": "choice",
                  "instructions": "Which team should handle this ticket?",
                  "options": {
                    "billing": "Payment or subscription issues",
                    "sales": "Pricing or account questions",
                    "technical": "Bugs or integration problems"
                  }
                },
                "frustration": {
                  "type": "score",
                  "instructions": "How frustrated the customer appears",
                  "levels": [
                    "Calm, just stating facts",
                    "Frustrated but civil",
                    "Very angry, strong language"
                  ]
                },
                "is_urgent": {
                  "type": "noul",
                  "instructions": "The message conveys urgency or time-sensitivity"
                }
              },
              "outputs": [
                {
                  "kind": "choice",
                  "name": "team",
                  "from": "department",
                  "map": {"billing": "Billing", "sales": "Sales", "technical": "Support"},
                  "min_confidence": 0,
                  "on_unknown": "error",
                  "required": false,
                  "emit_confidence": true
                },
                {"kind": "noul", "name": "urgent", "from": "is_urgent", "threshold": 0.5, "required": false},
                {"kind": "score", "name": "frustration", "from": "frustration", "required": false}
              ],
              "repeat": {
                "id_prefix": "issue:",
                "template": {
                  "type": "noul",
                  "instructions": "Does this ticket match the known issue in the item?"
                },
                "output": {"kind": "noul", "name": "matches", "threshold": 0.5}
              }
            }
            """.trimIndent()
        val state =
            """
            {
              "ticket": "Hi, I've been trying to connect my Stripe account for 3 days and it keeps failing. I'm losing sales. Please help ASAP.",
              "issues": [
                {"id": "stripe-connect", "summary": "Stripe account connection fails"},
                {"id": "password-reset", "summary": "Password reset email never arrives"},
                {"id": "invoice-export", "summary": "Invoice export produces an empty file"}
              ]
            }
            """.trimIndent()
        val items =
            """
            [
              {"id": "stripe-connect"},
              {"id": "password-reset"},
              {"id": "invoice-export"}
            ]
            """.trimIndent()
        typesafeClient(key).use { client ->
            val raw = client.judge(config, state, items)
            val verdict = Json.parseToJsonElement(raw).jsonObject
            println("team: ${verdict["team"]}")
            println("team confidence: ${verdict["team_confidence"]}")
            println("urgent: ${verdict["urgent"]}")
            println("frustration: ${verdict["frustration"]}")
            println("matches: ${verdict["matches"]}")
            assertTrue(verdict.isNotEmpty())
        }
    }

    @Test
    fun example44_multi_judge() {
        val openai = exampleApiKey()
        val typesafe = System.getenv("TYPESAFE_API_KEY").orEmpty()
        if (openai.isEmpty() || typesafe.isEmpty()) {
            println("Set OPENAI_API_KEY and TYPESAFE_API_KEY to run example 44.")
            return
        }
        val ticket =
            "Hi, I've been trying to connect my Stripe account for 3 days and it keeps failing. " +
                "I'm losing sales. Please help ASAP."
        val baseUrl = baseUrlFor(openai, System.getenv("OPENAI_BASE_URL"))
        openExampleClient {
            it.copy(baseUrl = baseUrl, apiKeys = mapOf("typesafe" to typesafe))
        }.use { client ->
            println("Three judges score one draft. Safety chooses hold or reply.")
            println("Ticket: $ticket")
            println("Running the graph...")
            val ticketJson = Json.encodeToString(String.serializer(), ticket)
            val answer = client.runGraph(deskSpec(exampleModel()), """{"ticket":$ticketJson}""")
            println("Answer: $answer")
            assertTrue(answer.isNotBlank() && answer != "null")
        }
    }

    private fun typesafeClient(key: String): SuperglueClient =
        SuperglueClient.open(
            ClientConfig(
                apiKey = "unused",
                apiKeys = mapOf("typesafe" to key),
            ),
        )

    private fun deskSpec(model: String): String {
        val encoded = Json.encodeToString(String.serializer(), model)
        val writer =
            """
            {
              "name": "desk",
              "persona": "a careful support agent",
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
        return """
            {
              "name": "desk",
              "require": ["ticket"],
              "output": "answer",
              "nodes": [
                {
                  "id": "draft",
                  "visits": 1,
                  "step": {
                    "kind": "agent",
                    "spec": $writer,
                    "prompt": "Write a support reply to this ticket. Use at most 4 sentences. Do not invent a refund or a delivery date.\n{ticket}"
                  }
                },
                {
                  "id": "safety",
                  "visits": 1,
                  "depends": ["draft"],
                  "step": {
                    "kind": "judge",
                    "input": "draft",
                    "config": {
                      "id": "desk.safety",
                      "model": "typesafe:jev-latest",
                      "timeout_ms": 400,
                      "on_error": "error",
                      "questions": {
                        "safe": {
                          "type": "noul",
                          "instructions": "The reply does not invent a refund, an account balance, or a delivery date"
                        }
                      },
                      "outputs": [{
                        "kind": "noul",
                        "name": "pass",
                        "from": "safe",
                        "threshold": 0.5,
                        "required": false
                      }]
                    }
                  },
                  "when": [{"path": "pass", "equals": true, "target": "reply"}],
                  "otherwise": "hold"
                },
                {
                  "id": "tone",
                  "visits": 1,
                  "depends": ["draft"],
                  "step": {
                    "kind": "judge",
                    "input": "draft",
                    "config": {
                      "id": "desk.tone",
                      "model": "typesafe:jev-latest",
                      "timeout_ms": 400,
                      "on_error": "error",
                      "questions": {
                        "tone": {
                          "type": "choice",
                          "instructions": "Which tone does this reply use?",
                          "options": {
                            "calm": "Polite and specific",
                            "terse": "Short but not rude",
                            "rude": "Blames the customer or sounds angry"
                          }
                        }
                      },
                      "outputs": [{
                        "kind": "choice",
                        "name": "label",
                        "from": "tone",
                        "map": {"calm": "calm", "terse": "terse", "rude": "rude"},
                        "min_confidence": 0,
                        "on_unknown": "error",
                        "required": false,
                        "emit_confidence": true
                      }]
                    }
                  }
                },
                {
                  "id": "coverage",
                  "visits": 1,
                  "depends": ["draft"],
                  "step": {
                    "kind": "judge",
                    "input": "draft",
                    "config": {
                      "id": "desk.coverage",
                      "model": "typesafe:jev-latest",
                      "timeout_ms": 400,
                      "on_error": "error",
                      "questions": {
                        "answers": {
                          "type": "noul",
                          "instructions": "The reply addresses the customer's request"
                        }
                      },
                      "outputs": [{
                        "kind": "noul",
                        "name": "pass",
                        "from": "answers",
                        "threshold": 0.5,
                        "required": false
                      }]
                    }
                  }
                },
                {
                  "id": "hold",
                  "write": "answer",
                  "visits": 1,
                  "step": {
                    "kind": "map",
                    "template": "We need a moment to check this ticket. A person will follow up."
                  }
                },
                {
                  "id": "reply",
                  "write": "answer",
                  "visits": 1,
                  "depends": ["tone", "coverage"],
                  "step": {
                    "kind": "agent",
                    "spec": $writer,
                    "prompt": "Ticket:\n{ticket}\n\nDraft:\n{draft}\n\nSafety:\n{safety}\n\nTone:\n{tone}\n\nCoverage:\n{coverage}\n\nWrite the reply the customer should receive. Use at most 4 sentences. If the tone label is rude, rewrite the draft in a calm tone. If coverage.pass is false, add the missing answer. Do not invent a refund or a delivery date."
                  }
                }
              ]
            }
            """.trimIndent()
    }

    private fun audioFile(): File {
        val override = System.getenv("SUPERGLUE_AUDIO_FILE").orEmpty()
        if (override.isNotEmpty()) {
            return File(override)
        }
        return listOf(
            File("examples/sample.mp3"),
            File("../examples/sample.mp3"),
            File("../../examples/sample.mp3"),
        ).firstOrNull { it.isFile } ?: File("../../examples/sample.mp3")
    }
}
