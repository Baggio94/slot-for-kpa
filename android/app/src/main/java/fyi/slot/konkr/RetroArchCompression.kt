package fyi.slot.konkr

import java.io.ByteArrayOutputStream
import java.util.zip.Inflater
import java.util.zip.Deflater

/** RetroArch libretro-common rzip_stream.c chunk format, strict bounded v1
 * (zlib) decoding. RZIP v2 is zstd, not Android's built-in zlib; preserve it
 * untouched until a tested zstd decoder is available.
 */
internal object RetroArchCompression {
    private val marker = byteArrayOf(35,82,90,73,80,118) // #RZIPv
    private const val LIMIT = 65 * 1024 * 1024

    fun isRzip(data: ByteArray): Boolean =
        data.size >= 20 && marker.indices.all { data[it] == marker[it] } &&
        data[7] == 35.toByte()

    /** RetroArch #RZIPv1# zlib writer: 128 KiB independent chunks.
     *  Preserve the format of the existing KONKR .state.auto files.
     */
    fun encode(data: ByteArray): ByteArray {
        require(data.isNotEmpty() && data.size <= LIMIT) {
            "RetroArch state outside supported size limit"
        }
        val chunkSize = 131_072
        val out = ByteArrayOutputStream(data.size / 2 + 64)
        out.write(byteArrayOf(35,82,90,73,80,118,1,35)) // #RZIPv1#
        fun write32(value: Long) {
            require(value in 0..0xffff_ffffL)
            for (i in 0..3) out.write(((value ushr (i * 8)) and 255).toInt())
        }
        write32(chunkSize.toLong())
        write32(data.size.toLong())
        write32(0)
        var offset = 0
        while (offset < data.size) {
            val len = minOf(chunkSize, data.size - offset)
            val deflater = Deflater(6, false)
            val compressed = ByteArrayOutputStream(len / 2 + 64)
            try {
                deflater.setInput(data, offset, len)
                deflater.finish()
                val buffer = ByteArray(65536)
                while (!deflater.finished()) {
                    val n = deflater.deflate(buffer)
                    check(n > 0) { "RetroArch compression stalled" }
                    compressed.write(buffer, 0, n)
                    check(compressed.size() <= 2 * chunkSize) {
                        "Oversized compressed RetroArch chunk"
                    }
                }
            } finally {
                deflater.end()
            }
            val chunk = compressed.toByteArray()
            require(chunk.isNotEmpty())
            write32(chunk.size.toLong())
            out.write(chunk)
            offset += len
        }
        val result = out.toByteArray()
        check(decode(result).contentEquals(data)) {
            "Compressed RetroArch state failed its round trip"
        }
        return result
    }

    /** Refuse overwriting unknown, legacy raw, v2 or corrupt external states.
     * A valid header alone is insufficient: require a complete MEM and END
     * block before replacing a RetroArch state.
     */
    fun requireSupportedContainer(data: ByteArray) {
        val decoded = decode(data)
        require(decoded.size >= 24 && decoded.copyOfRange(0, 8)
            .contentEquals(byteArrayOf(82,65,83,84,65,84,69,1))) {
            "Existing RetroArch state format is unsupported; preserved"
        }
        fun size32(pos: Int): Long =
            (0..3).fold(0L) { v, i ->
                v or ((decoded[pos + i].toLong() and 255L) shl (8 * i))
            }
        var offset = 8
        var memoryFound = false
        while (offset + 8 <= decoded.size) {
            val tag = decoded.copyOfRange(offset, offset + 4)
                .toString(Charsets.US_ASCII)
            val payloadSize = size32(offset + 4)
            val end = offset.toLong() + 8L + payloadSize
            require(end <= decoded.size.toLong()) {
                "Truncated RetroArch container; preserved"
            }
            when (tag) {
                "MEM " -> {
                    require(!memoryFound && payloadSize in 1..(64L * 1024 * 1024)) {
                        "Invalid RetroArch memory block; preserved"
                    }
                    memoryFound = true
                }
                "END " -> {
                    require(memoryFound && payloadSize == 0L && end == decoded.size.toLong()) {
                        "Incomplete RetroArch container; preserved"
                    }
                    return
                }
            }
            offset = ((end + 7) and -8L).toInt()
        }
        error("Missing RetroArch END block; preserved")
    }

    fun decode(data: ByteArray): ByteArray {
        if (!isRzip(data)) return data
        val version=data[6].toInt() and 255
        require(version==1) {
            if (version==2) "RetroArch RZIP zstd states are not supported yet"
            else "Unknown RetroArch RZIP version"
        }
        fun readLE32(offset:Int):Long =
            (0..3).fold(0L){total,k->total or ((data[offset+k].toLong() and 255L) shl (8*k))}
        val block=readLE32(8)
        val total=readLE32(12) or (readLE32(16) shl 32)
        require(block in 1..(64L*1024*1024)) {"Invalid RZIP block size"}
        require(total in 1..LIMIT.toLong()) {"Invalid RZIP uncompressed size"}
        val output=ByteArrayOutputStream(total.toInt())
        var offset=20
        while(output.size()<total) {
            require(offset+4<=data.size) {"Truncated RetroArch RZIP chunk"}
            val compressed=readLE32(offset)
            offset+=4
            require(compressed in 1..(block*2) && offset+compressed<=data.size) {
                "Invalid RetroArch RZIP compressed chunk size"
            }
            val inflater=Inflater(false)  // independent zlib stream per chunk
            try {
                inflater.setInput(data,offset,compressed.toInt())
                val expected=minOf(block.toInt(),total.toInt()-output.size())
                val buffer=ByteArray(expected)
                var written=0
                while(written<expected && !inflater.finished()) {
                    val n=inflater.inflate(buffer,written,expected-written)
                    if(n==0) {
                        require(!inflater.needsDictionary() && !inflater.needsInput()) {
                            "Malformed RZIP zlib chunk"
                        }
                        error("RetroArch inflater made no progress")
                    }
                    written+=n
                }
                require(written==expected && inflater.finished() && inflater.remaining==0) {
                    "Incomplete or oversized RetroArch RZIP chunk"
                }
                output.write(buffer)
            } finally {
                inflater.end()
            }
            offset+=compressed.toInt()
        }
        require(offset==data.size) {"Unexpected trailing RZIP data"}
        return output.toByteArray()
    }
}
