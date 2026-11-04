package com.superglue.kt.examples

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Documents which numbered superglue-py/js examples have Kotlin JUnit coverage.
 * Live sections require OPENAI_API_KEY.
 * Rows 33, 38, and 39 stay in the Rust examples.
 */
class ExamplesRunnerTest {
    private val coveredExamples = (1..32).toList() + listOf(34, 35, 36, 37)

    @Test
    fun documentsCoveredExampleNumbers() {
        assertEquals((1..32).toList() + listOf(34, 35, 36, 37), coveredExamples)
    }
}
