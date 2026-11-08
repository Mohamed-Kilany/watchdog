package app.watchdog.remote.protocol

import org.json.JSONObject
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.io.File
import java.util.Random

/**
 * Kotlin conformance against the committed golden vectors (spec §7). Mirrors
 * `crates/protocol/tests/golden_vectors.rs` so both implementations are held
 * to the same contract.
 */
class GoldenVectorTest {
    private val vectorsDir = locateVectorsDir()

    @Test
    fun `valid vectors decode and re-encode byte exact`() {
        val vectors = manifest().getJSONArray("vectors")
        assertTrue("expected at least 14 golden vectors", vectors.length() >= 14)

        for (index in 0 until vectors.length()) {
            val entry = vectors.getJSONObject(index)
            val file = entry.getString("file")
            val expectedType = entry.getString("type")
            val bytes = File(vectorsDir, file).readBytes()

            val packets = StreamDecoder().feed(bytes)
            assertEquals("$file: expected exactly one packet", 1, packets.size)
            val packet = packets.single()
            assertEquals("$file: message type", expectedType, packet.type.name)

            MessageCodec.validate(packet)
            assertArrayEquals("$file: re-encode mismatch", bytes, PacketCodec.encode(packet))
        }
    }

    @Test
    fun `valid vectors survive byte at a time feeding`() {
        val vectors = manifest().getJSONArray("vectors")
        for (index in 0 until vectors.length()) {
            val file = vectors.getJSONObject(index).getString("file")
            val bytes = File(vectorsDir, file).readBytes()
            val decoder = StreamDecoder()
            val packets = mutableListOf<Packet>()
            for (byte in bytes) {
                packets += decoder.feed(byteArrayOf(byte))
            }
            assertEquals("$file: byte-at-a-time packet count", 1, packets.size)
        }
    }

    @Test
    fun `invalid vectors fail without panicking`() {
        val invalid = manifest().getJSONArray("invalid")
        for (index in 0 until invalid.length()) {
            val entry = invalid.getJSONObject(index)
            val file = entry.getString("file")
            val expect = entry.getString("expect")
            val bytes = File(vectorsDir, file).readBytes()
            when (expect) {
                "NeedMore" -> assertTrue("$file: expected NeedMore", StreamDecoder().feed(bytes).isEmpty())
                "MessageDecode" -> assertTypedDecodeFails(file, bytes)
                else -> assertFramingFails(file, bytes, expect)
            }
        }
    }

    @Test
    fun `concatenated frames are split correctly`() {
        val ping = File(vectorsDir, "0012-ping.bin").readBytes()
        val pong = File(vectorsDir, "0013-pong.bin").readBytes()
        val stream = ping + pong

        val packets = StreamDecoder().feed(stream)
        assertEquals(2, packets.size)
        assertEquals(MessageType.PING, packets[0].type)
        assertEquals(MessageType.PONG, packets[1].type)
    }

    @Test
    fun `random chunk splits and disconnects never panic`() {
        val frame = File(vectorsDir, "0003-frame.bin").readBytes()
        val random = Random(RANDOM_SEED)
        repeat(CHAOS_CASES) {
            val decoder = StreamDecoder()
            var offset = 0
            while (offset < frame.size) {
                val chunk = minOf(frame.size - offset, 1 + random.nextInt(MAX_CHUNK))
                val packets = decoder.feed(frame.copyOfRange(offset, offset + chunk))
                assertTrue("at most one packet per partial feed", packets.size <= 1)
                offset += chunk
                if (random.nextInt(DISCONNECT_ONE_IN) == 0) {
                    break // simulated disconnect mid-stream
                }
            }
        }
        assertEquals(1, StreamDecoder().feed(frame).size)
    }

    @Test
    fun `garbage inputs never panic`() {
        val random = Random(RANDOM_SEED + 1)
        val hello = File(vectorsDir, "0001-host-hello.bin").readBytes()
        repeat(CHAOS_CASES) { case ->
            val garbage = ByteArray(random.nextInt(MAX_GARBAGE))
            random.nextBytes(garbage)
            try {
                StreamDecoder().feed(garbage)
            } catch (e: ProtocolException) {
                // Expected for structural garbage.
            }
            if (case % 7 == 0) {
                val decoder = StreamDecoder()
                decoder.feed(hello.copyOfRange(0, 7))
                try {
                    decoder.feed(garbage)
                } catch (e: ProtocolException) {
                    // Expected.
                }
            }
        }
    }

    private fun assertTypedDecodeFails(file: String, bytes: ByteArray) {
        val packets = StreamDecoder().feed(bytes)
        assertEquals("$file: expected one framed packet", 1, packets.size)
        try {
            MessageCodec.validate(packets.single())
            fail("$file: expected typed decode failure")
        } catch (e: ProtocolException) {
            assertEquals("$file: wrong typed error", ProtocolError.MessageDecode, e.error)
        }
    }

    private fun assertFramingFails(file: String, bytes: ByteArray, expect: String) {
        try {
            StreamDecoder().feed(bytes)
            fail("$file: expected $expect")
        } catch (e: ProtocolException) {
            assertEquals("$file: wrong framing error", expect, e.error.name)
        }
    }

    private fun manifest(): JSONObject = JSONObject(File(vectorsDir, "manifest.json").readText())

    private companion object {
        const val RANDOM_SEED = 0x5EED
        const val CHAOS_CASES = 2_000
        const val MAX_CHUNK = 64
        const val DISCONNECT_ONE_IN = 10
        const val MAX_GARBAGE = 512

        fun locateVectorsDir(): File {
            var dir = File(System.getProperty("user.dir")).absoluteFile
            while (dir.parentFile != null) {
                val candidate = File(dir, "protocol/vectors")
                if (File(candidate, "manifest.json").isFile) {
                    return candidate
                }
                dir = dir.parentFile
            }
            throw AssertionError("protocol/vectors not found from ${System.getProperty("user.dir")}")
        }
    }
}
