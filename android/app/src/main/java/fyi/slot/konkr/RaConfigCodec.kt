package fyi.slot.konkr

/**
 * Strict serializer for the app-private configuration consumed by the Rust
 * RetroAchievements worker. Credentials never belong in ROM/SAF/shared folders.
 * This is intentionally Kotlin/JVM-only so it can be regression-tested in CI.
 */
internal object RaConfigCodec {
    private fun tomlString(value: String): String {
        require(value.isNotBlank() && value.length <= 512) { "Invalid account value" }
        require(value.none { it.isISOControl() }) { "Control characters are not allowed" }
        return buildString {
            append('"')
            for (char in value) {
                when (char) {
                    '\\' -> append("\\\\")
                    '"' -> append("\\\"")
                    else -> append(char)
                }
            }
            append('"')
        }
    }

    fun configuredAccount(username: String, token: String): String {
        require(username.length <= 100 && token.length <= 512)
        return buildString {
            append("enabled = true\n")
            append("username = ").append(tomlString(username)).append('\n')
            append("token = ").append(tomlString(token)).append('\n')
            // The login password is used only for one HTTPS exchange, never persisted.
            append("password = \"\"\n")
        }
    }

    /** The display label is never an authentication proof. */
    fun displayedUsername(config: String): String? {
        val line = config.lineSequence().firstOrNull { it.startsWith("username = ") }
            ?: return null
        val source = line.removePrefix("username = ").trim()
        if (!source.startsWith('"') || !source.endsWith('"') || source.length < 3) return null
        val body = source.substring(1, source.length - 1)
        val out = StringBuilder()
        var cursor = 0
        while (cursor < body.length) {
            val c = body[cursor++]
            if (c != '\\') {
                out.append(c)
            } else {
                if (cursor >= body.length) return null
                when (body[cursor++]) {
                    '\\' -> out.append('\\')
                    '"' -> out.append('"')
                    else -> return null
                }
            }
        }
        return out.toString().takeIf { it.isNotBlank() }
    }
}
