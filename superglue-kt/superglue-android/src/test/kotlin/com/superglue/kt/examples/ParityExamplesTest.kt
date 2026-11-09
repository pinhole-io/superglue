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
 * JUnit counterparts for examples 38 and 40–42.
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
                "threshold": 0.5
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
                  "emit_confidence": true
                },
                {"kind": "noul", "name": "urgent", "from": "is_urgent", "threshold": 0.5},
                {"kind": "score", "name": "frustration", "from": "frustration"}
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

    private fun typesafeClient(key: String): SuperglueClient =
        SuperglueClient.open(
            ClientConfig(
                apiKey = "unused",
                apiKeys = mapOf("typesafe" to key),
            ),
        )

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
