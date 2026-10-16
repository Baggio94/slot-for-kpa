package fyi.slot.konkr

import android.content.Context
import android.media.AudioAttributes
import android.media.SoundPool
import android.util.Log
import java.io.File
import java.io.FileOutputStream

/** Play the two original Slot 48-kHz mono 16-bit LE cartridge recordings. */
internal class CartSounds(context: Context) {
    companion object {
        const val INSERT = 1
        const val EJECT = 2
        private const val RATE = 48_000
        // Slight increase over the previous 0.85 without changing game audio.
        private const val CART_SFX_VOLUME = 0.98f
    }
    private val soundPool = SoundPool.Builder().setMaxStreams(2)
        .setAudioAttributes(AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_GAME)
            .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION).build())
        .build()
    private val samples = IntArray(3)
    private val loaded = HashSet<Int>()
    @Volatile private var closed = false

    init {
        soundPool.setOnLoadCompleteListener { _, id, result ->
            if (result == 0) synchronized(loaded) { loaded.add(id) }
            else Log.w("SlotKonkr", "SoundPool loading failed: " + id + " (" + result + ")")
        }
        for ((kind, name) in arrayOf(INSERT to "insert", EJECT to "eject")) {
            try {
                samples[kind] = soundPool.load(makeWav(context, name).absolutePath, 1)
            } catch (error: Exception) {
                Log.w("SlotKonkr", "Failed to load original Slot sound: " + name, error)
            }
        }
    }

    fun play(kind: Int) {
        if (closed || kind !in INSERT..EJECT) return
        val id = samples[kind]
        if (id == 0 || !synchronized(loaded) { loaded.contains(id) }) return
        soundPool.play(id, CART_SFX_VOLUME, CART_SFX_VOLUME, 1, 0, 1f)
    }

    fun pause() { if (!closed) soundPool.autoPause() }
    fun resume() { if (!closed) soundPool.autoResume() }

    fun release() {
        closed = true
        soundPool.release()
        synchronized(loaded) { loaded.clear() }
    }

    private fun makeWav(context: Context, name: String): File {
        val pcm = context.assets.open(name + ".pcm").use { it.readBytes() }
        require(pcm.isNotEmpty() && pcm.size % 2 == 0) { "Invalid Slot PCM asset" }
        val output = File(context.cacheDir, "original-slot-" + name + "-" + pcm.size + ".wav")
        if (output.isFile && output.length() == pcm.size.toLong() + 44L) return output
        val temporary = File(context.cacheDir, "original-slot-" + name + ".tmp")
        try {
            FileOutputStream(temporary).use { out ->
                out.write("RIFF".toByteArray(Charsets.US_ASCII))
                out.u32(pcm.size + 36)
                out.write("WAVEfmt ".toByteArray(Charsets.US_ASCII))
                out.u32(16)
                out.u16(1) // signed PCM
                out.u16(1) // mono
                out.u32(RATE)
                out.u32(RATE * 2)
                out.u16(2)
                out.u16(16)
                out.write("data".toByteArray(Charsets.US_ASCII))
                out.u32(pcm.size)
                out.write(pcm)
            }
            check(temporary.renameTo(output)) { "Could not prepare WAV" }
        } finally {
            temporary.delete()
        }
        return output
    }

    private fun FileOutputStream.u16(value: Int) {
        write(value and 255)
        write((value ushr 8) and 255)
    }
    private fun FileOutputStream.u32(value: Int) {
        u16(value and 65535)
        u16(value ushr 16)
    }
}
