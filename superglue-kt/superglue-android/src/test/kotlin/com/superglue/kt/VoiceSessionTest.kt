package com.superglue.kt

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.net.InetAddress
import java.net.ServerSocket
import java.security.MessageDigest
import java.util.Base64
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Realtime voice binding tests. No test calls a provider. */
class VoiceSessionTest {
    private fun client(): SuperglueClient =
        SuperglueClient.open(ClientConfig(apiKey = "sk-test", model = "gpt-4o-mini"))

    private fun connectError(configJson: String): RuntimeException {
        client().use { c ->
            try {
                c.voiceConnect(configJson) { _, _ -> }.close()
            } catch (e: RuntimeException) {
                return e
            }
        }
        fail("voiceConnect accepted $configJson")
        error("unreachable")
    }

    /** Accept one WebSocket upgrade, then drain frames until the peer closes. */
    private fun localWebSocket(): ServerSocket {
        val listener = ServerSocket(0, 1, InetAddress.getLoopbackAddress())
        Thread {
            listener.accept().use { conn ->
                val input = conn.getInputStream()
                val request = StringBuilder()
                while (!request.endsWith("\r\n\r\n")) {
                    val b = input.read()
                    if (b < 0) return@Thread
                    request.append(b.toChar())
                }
                val key =
                    request.lines()
                        .first { it.lowercase().startsWith("sec-websocket-key") }
                        .substringAfter(':')
                        .trim()
                val digest =
                    MessageDigest.getInstance("SHA-1")
                        .digest((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").toByteArray())
                val accept = Base64.getEncoder().encodeToString(digest)
                conn.getOutputStream().write(
                    (
                        "HTTP/1.1 101 Switching Protocols\r\n" +
                            "Upgrade: websocket\r\nConnection: Upgrade\r\n" +
                            "Sec-WebSocket-Accept: $accept\r\n\r\n"
                    ).toByteArray(),
                )
                val buf = ByteArray(4096)
                while (input.read(buf) >= 0) {
                    // Drain client frames.
                }
            }
        }.apply { isDaemon = true }.start()
        return listener
    }

    @Test
    fun rejectsUnsupportedProvider() {
        val e = connectError("""{"provider":"anthropic"}""")
        assertTrue(e.message.orEmpty(), e.message.orEmpty().contains("supported provider"))
    }

    @Test
    fun openAiRequiresRealtimeModel() {
        val e = connectError("""{"provider":"openai"}""")
        assertTrue(e.message.orEmpty(), e.message.orEmpty().contains("model"))
    }

    @Test
    fun configRejectsApiKeyField() {
        val e = connectError("""{"provider":"openai","model":"gpt-realtime","api_key":"sk-leak"}""")
        assertFalse(e.message.orEmpty().contains("sk-leak"))
    }

    @Test
    fun closedSessionRejectsCommands() {
        val events = CopyOnWriteArrayList<String>()
        val closed = CountDownLatch(1)
        localWebSocket().use { listener ->
            val url = "ws://127.0.0.1:${listener.localPort}"
            client().use { c ->
                val session =
                    c.voiceConnect(
                        """{"provider":"openai","model":"gpt-realtime","base_url":"$url"}""",
                    ) { eventJson, _ ->
                        events.add(eventJson)
                        if (eventJson.contains("\"closed\"")) closed.countDown()
                    }
                session.sendAudio(byteArrayOf(0, 0))
                session.close()
                assertTrue(events.toString(), closed.await(5, TimeUnit.SECONDS))
                try {
                    session.sendText("too late")
                    fail("closed session accepted a command")
                } catch (e: RuntimeException) {
                    assertTrue(e.message.orEmpty(), e.message.orEmpty().contains("voice session"))
                }
            }
        }
        assertTrue(events.first(), events.first().contains("session_created"))
        assertEquals(1, events.count { it.contains("\"closed\"") })
    }
}
