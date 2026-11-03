package com.superglue.kt.examples

import com.superglue.kt.ClientConfig
import com.superglue.kt.JsonCallback
import com.superglue.kt.StatusEmitter
import com.superglue.kt.SuperglueClient
import org.junit.Assert.assertTrue
import kotlin.math.min
import kotlin.math.pow

internal fun exampleApiKey(): String = System.getenv("OPENAI_API_KEY").orEmpty()

internal fun exampleModel(): String =
    System.getenv("OPENAI_MODEL") ?: "gpt-6-luna"

internal inline fun runLive(number: Int, block: () -> Unit) {
    if (exampleApiKey().isEmpty()) {
        println("Set OPENAI_API_KEY to run example $number live.")
        return
    }
    block()
}

internal fun baseUrlFor(apiKey: String, override: String?): String {
    val trimmed = override?.trim().orEmpty()
    if (trimmed.isNotEmpty()) {
        return trimmed
    }
    return if (apiKey.startsWith("sk-")) {
        "https://api.openai.com"
    } else {
        "http://localhost:11434"
    }
}

internal fun backoffSchedule(
    initialMs: Int,
    multiplier: Double,
    maxMs: Int,
    retries: Int,
): List<Int> =
    (0 until retries).map { attempt ->
        val delay = initialMs * multiplier.pow(attempt)
        min(delay, maxMs.toDouble()).toInt()
    }

internal fun openExampleClient(
    apiKey: String = exampleApiKey(),
    model: String = exampleModel(),
    emitter: StatusEmitter? = null,
    adjust: (ClientConfig) -> ClientConfig = { it },
): SuperglueClient {
    val config = adjust(ClientConfig(apiKey = apiKey, model = model))
    return SuperglueClient.open(config, statusEmitter = emitter)
}

internal fun assertHasContent(content: String?) {
    assertTrue(content?.isNotEmpty() == true)
}

internal fun SuperglueClient.registerContextTools() {
    registerTool(
        "get_weather",
        "Get weather for a city.",
        """{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}""",
        JsonCallback { _ -> """{"city":"Paris","temp_f":72,"condition":"sunny"}""" },
    )
    registerTool(
        "calculate",
        "Evaluate a math expression.",
        """{"type":"object","properties":{"expression":{"type":"string"}},"required":["expression"]}""",
        JsonCallback { _ -> """{"expression":"1+1","result":42}""" },
    )
    registerTool(
        "get_time",
        "Return current UTC time.",
        """{"type":"object","properties":{}}""",
        JsonCallback { _ -> """{"time":"12:00Z"}""" },
        staticTool = true,
    )
}
