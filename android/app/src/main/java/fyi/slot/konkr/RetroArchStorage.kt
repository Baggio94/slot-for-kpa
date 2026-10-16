package fyi.slot.konkr

import android.content.Context
import android.net.Uri
import android.os.SystemClock
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.util.Log
import java.io.File
import java.io.FileNotFoundException
import java.security.MessageDigest
import java.util.Locale

/**
 * RetroArch shared saves for "Sort Saves/States Into Folders By Core Name".
 * Operates only on user-approved READ+WRITE SAF tree roots, on a worker thread.
 * No direct /storage/emulated/0 or /Android/data paths are assumed.
 *
 * Layout: <chosen saves root>/mGBA/Game Name.srm or /gpSP/Game Name.srm
 *         <chosen states root>/mGBA/Game Name.state.auto (or gpSP).
 *
 * Never silently overwrite an external compressed state. Before replacing
 * existing saves, retain a separate untouched copy in SlotBackup/ as rollback.
 */
internal object RetroArchStorage {
    private const val TAG = "SlotKonkr"
    private const val MAX_SAVE = 2 * 1024 * 1024
    class SaveConflict(message: String) : IllegalStateException(message)
    private const val MAX_STATE = 65 * 1024 * 1024

    data class Target(
        val romUri: String,
        val stem: String,
        val core: String,
        val saveLocal: File,
        val rtcLocal: File,
        val stateDefaultLocal: File,
        val manualExportPrefix: File,
        val stateImportLocal: File,
        val stateExportLocal: File,
        val savesTree: Uri?,
        val statesTree: Uri?,
        val isGba: Boolean,
        val romIdentity: String,
        val shareGbaSaves: Boolean = false
    )

    fun target(context: Context, rawUri: String, core: String, savesTree: Uri?, statesTree: Uri?,
               scannedFileName: String? = null,
               shareGbaSaves: Boolean = false): Target {
        require(core == "mGBA" || core == "gpSP")
        // ROM discovery (or restored shelf cache) already queried the SAF
        // display name. Reuse only that exact URI's name; older caches and
        // direct launches still query ContentResolver as a safe fallback.
        val name = scannedFileName?.takeIf { candidate ->
            candidate.endsWith(".gba", true) || candidate.endsWith(".gbc", true) ||
                candidate.endsWith(".gb", true)
        } ?: queryRomName(context, Uri.parse(rawUri))
        val stem = name.substringBeforeLast('.', name)
            .trim().take(200)
        require(stem.isNotEmpty() && stem !in setOf(".", "..") &&
            stem.none { it == '/' || it == '\\' || it.code < 32 })
        val digest = MessageDigest.getInstance("SHA-256")
            .digest(rawUri.toByteArray(Charsets.UTF_8))
            .joinToString("") { "%02x".format(Locale.ROOT, it.toInt() and 255) }
        val prefix = File(context.filesDir, "Saves").apply { mkdirs() }
        val coreFolder = File(prefix, core).apply { mkdirs() }
        val defaultStates = File(File(context.filesDir, "States"), core).apply { mkdirs() }
        val coreLower = if (core == "mGBA") "mgba" else "gpsp"
        return Target(rawUri, stem, core,
            File(coreFolder, "$stem.srm"),
            File(coreFolder, "$stem.rtc"),
            File(defaultStates, "$stem.state.auto"),
            File(File(prefix, "ManualExports").apply { mkdirs() }, "$digest.$coreLower"),
            File(prefix, "$digest.$coreLower.retroarch-import.state.auto"),
            File(prefix, "$digest.$coreLower.retroarch-export.state.auto"),
            savesTree, statesTree,
            name.endsWith(".gba", ignoreCase = true), digest, shareGbaSaves)
    }

    /** The choice only exists on an actual mGBA<->gpSP switch with
     * Share Saves enabled and an existing auto-state for the destination
     * core. A same-core launch must never show this dialog.
     */
    internal fun shouldOfferCoreResume(
        sharing: Boolean, previousCore: String?, incomingCore: String,
        hasDestinationState: Boolean
    ): Boolean = sharing && hasDestinationState &&
        previousCore in setOf("mGBA", "gpSP") && previousCore != incomingCore &&
        incomingCore in setOf("mGBA", "gpSP")

    /** Only auto-states count; manual Polaroids do not implicitly resume. */
    fun hasCoreAutoState(context: Context, target: Target): Boolean {
        val lower = if (target.core == "mGBA") "mgba" else "gpsp"
        val local = File(context.filesDir,
            "Saves/${target.romIdentity}.$lower.state")
        if ((local.isFile && local.length() > 0) ||
            (target.stateImportLocal.isFile && target.stateImportLocal.length() > 0) ||
            (target.stateDefaultLocal.isFile && target.stateDefaultLocal.length() > 0)) {
            return true
        }
        val index = File(context.filesDir,
            "Saves/${target.romIdentity}.$lower.state-generation.active")
        if (index.isFile) {
            val key = index.readText()
            if (key.startsWith("${target.romIdentity}.$lower.gen-") &&
                key.matches(Regex("[a-zA-Z0-9.-]{1,180}")) &&
                File(context.filesDir, "Saves/$key.state").isFile) return true
        }
        return target.statesTree?.let { root ->
            readNamed(context, root, target.core,
                target.stem + ".state.auto", MAX_STATE) != null
        } ?: false
    }

    /** User explicitly selected the destination core's existing auto-state.
     * Re-stage it from RetroArch, even when the normal automatic importer was
     * deferred to protect that old state while offering the choice.
     * Never delete the source or another emulator core's state.
     */
    fun importExplicitExistingState(context: Context, target: Target) {
        val bytes = if (target.statesTree != null) {
            readNamed(context, target.statesTree, target.core,
                target.stem + ".state.auto", MAX_STATE)
        } else {
            target.stateDefaultLocal.takeIf { it.isFile }?.readBytes()
        }
        if (bytes != null) {
            require(bytes.isNotEmpty() && bytes.size <= MAX_STATE) { "Existing state is invalid" }
            stage(target.stateImportLocal, RetroArchCompression.decode(bytes))
        }
    }

    fun importBeforeLaunch(context: Context, target: Target) {
        val started = SystemClock.elapsedRealtime()
        // The original implementation enumerated the entire save/state
        // directory SIX times per game launch. Large ROM collections with
        // Syncthing conflict files make SAF queries very expensive.
        // For Android's own ExternalStorageProvider, document IDs are
        // hierarchical. Use the user-granted tree to open the exact document
        // directly, without listing its siblings. Other providers retain the
        // existing safe directory-enumeration fallback.
        if (target.isGba && target.shareGbaSaves) {
            importSharedGba(context, target)
        } else {
            target.savesTree?.let { root ->
                readNamed(context, root, target.core, target.stem + ".srm", MAX_SAVE)
                    ?.let { stage(target.saveLocal, it) }
            }
        }
        target.savesTree?.let { root ->
            readNamed(context, root, target.core, target.stem + ".rtc", 4096)
                ?.let { bytes ->
                    require(bytes.isNotEmpty()) { "Empty RetroArch RTC" }
                    stage(target.rtcLocal, bytes)
                }
        }
        // When Share Saves chooses cartridge SRAM over an older same-core
        // Save State, preserve the original imported RetroArch state byte for
        // byte. Rust will resume/write only a fresh generation and the export
        // guard will keep the original external auto-state unchanged.
        // Do not even restage an existing private import during this launch.
        val coreLower = if (target.core == "mGBA") "mgba" else "gpsp"
        val guardRoot = File(context.filesDir, "Saves")
        val skipped = File(guardRoot,
            "${target.romIdentity}.$coreLower.skip-resume.once").isFile
        val stateProtected = File(guardRoot,
            "${target.romIdentity}.$coreLower.state-export-protected").isFile
        if (skipped || stateProtected) {
            Log.i(TAG, "Deferred stale RetroArch state import; SRAM / existing states preserved")
            return
        }
        val savesAt = SystemClock.elapsedRealtime()
        val incoming = if (target.statesTree != null) {
            readNamed(context, target.statesTree, target.core,
                target.stem + ".state.auto", MAX_STATE)
        } else if (target.stateDefaultLocal.isFile) {
            target.stateDefaultLocal.readBytes()
        } else null
        val readAt = SystemClock.elapsedRealtime()
        if (incoming != null) {
            try {
                // Rust validates RASTATE before passing the memory to libretro.
                stage(target.stateImportLocal, RetroArchCompression.decode(incoming))
            } catch (error: Exception) {
                Log.w(TAG, "RetroArch state not importable; private fallback preserved", error)
                target.stateImportLocal.delete()
            }
        } else {
            target.stateImportLocal.delete()
        }
        val doneAt = SystemClock.elapsedRealtime()
        Log.i(TAG, "RetroArch SAF import breakdown: saves=${savesAt - started}ms, " +
            "state read=${readAt - savesAt}ms, state decode/stage=${doneAt - readAt}ms, " +
            "total=${doneAt - started}ms")
    }

    /** Returns human-readable export errors; no silent data loss. */
    fun exportAfterSave(context: Context, target: Target): List<String> {
        val failures = mutableListOf<String>()
        if (target.isGba && target.shareGbaSaves) {
            try {
                exportSharedGba(context, target)
            } catch (e: Exception) {
                Log.e(TAG, "Shared GBA SRAM sync deferred; originals preserved", e)
                failures.add("Shared GBA save sync deferred: " + (e.message ?: "check existing saves"))
            }
        } else if (target.savesTree != null && target.saveLocal.isFile) {
            try {
                val data = target.saveLocal.readBytes()
                if (data.isNotEmpty() && data.size <= MAX_SAVE) {
                    writeSaf(context, target.savesTree, target.core,
                        target.stem + ".srm", data, isState = false)
                }
            } catch (e: Exception) {
                Log.e(TAG, "RetroArch SRAM export failed for ${target.core}/${target.stem}", e)
                failures.add("Save sync failed (${target.core}): " + (e.message ?: "access denied"))
            }
        }
        if (target.savesTree != null && target.rtcLocal.isFile) {
            try {
                val data = target.rtcLocal.readBytes()
                require(data.size in 1..4096) { "Invalid RTC size" }
                writeSaf(context, target.savesTree, target.core,
                    target.stem + ".rtc", data, isState = false)
            } catch (e: Exception) {
                Log.e(TAG, "RetroArch RTC export failed for ${target.core}/${target.stem}", e)
                failures.add("RTC sync failed (${target.core}): " + (e.message ?: "access denied"))
            }
        }
        // A core switch that chose shared SRAM intentionally did NOT
        // consume the previous core-specific auto-state. Its successor is
        // written to a separate private generation by Rust. Publishing it
        // over the original RetroArch .state.auto would silently destroy the
        // ignored state (F-Zero regression). Wait for explicit user consent.
        val lower = if (target.core == "mGBA") "mgba" else "gpsp"
        val guarded = File(context.filesDir,
            "Saves/${target.romIdentity}.$lower.state-export-protected").isFile
        if (!guarded && target.stateExportLocal.isFile) {
            try {
                val bytes = target.stateExportLocal.readBytes()
                require(bytes.size in 16..MAX_STATE && bytes.startsWithRASTATE()) {
                    "Invalid uncompressed RASTATE export"
                }
                // Keep the exact #RZIPv1# format used by this KONKR RetroArch.
                val compressed = RetroArchCompression.encode(bytes)
                if (target.statesTree != null) {
                    writeSaf(context, target.statesTree, target.core,
                        target.stem + ".state.auto", compressed, isState = true)
                } else {
                    // Private defaults use the same per-core RZIP layout as RetroArch.
                    stage(target.stateDefaultLocal, compressed)
                    check(target.stateDefaultLocal.readBytes().contentEquals(compressed)) {
                        "Default state verification failed"
                    }
                }
            } catch (e: Exception) {
                Log.e(TAG, "RetroArch state export failed for ${target.core}/${target.stem}", e)
                failures.add("State sync skipped (${target.core}): " + (e.message ?: "unsupported format"))
            }
        }
        if (guarded) {
            Log.i(TAG, "Preserved previous RetroArch auto-state; " +
                "new cross-core state is stored in a private generation")
        }
        return failures
    }

    /** Shared GBA battery RAM only. Save states and RTC remain core-specific.
     *  A new shared journal is recorded ONLY after both RetroArch mirrors were
     *  verified. If existing mGBA/gpSP saves disagree without a reliable
     *  common ancestor, NEVER choose a winner behind the user's back.
     */
    private fun sharedFiles(context: Context, target: Target): Pair<File, File> {
        val root = File(context.filesDir, "Saves/SharedGBA").apply { mkdirs() }
        val snapshot = File(root, target.romIdentity + ".srm")
        val loaded = File(root, target.romIdentity + ".loaded.srm")
        return snapshot to loaded
    }

    private fun otherCore(core: String) = if (core == "mGBA") "gpSP" else "mGBA"

    private fun otherPrivate(context: Context, target: Target) =
        File(context.filesDir, "Saves/${otherCore(target.core)}/${target.stem}.srm")

    private fun validGbaRam(data: ByteArray): Boolean {
        // GBA EEPROM 512B/8KiB, SRAM 32KiB, Flash 64KiB/128KiB.
        // Leave exotic sizes unmirrored until actually validated on both cores.
        return data.size in setOf(512, 8192, 32768, 65536, 131072)
    }

    /**
     * gpSP's pinned libretro core always exports 128 KiB, while mGBA exports
     * the cartridge's actual EEPROM/SRAM/Flash capacity. Treat only erased
     * 0xFF suffixes as equivalent: a non-erased suffix is real save data.
     * This does not compare save histories by timestamps and never chooses
     * between genuinely different prefixes.
     */
    internal fun sameGbaRam(a: ByteArray?, b: ByteArray?): Boolean {
        if (a == null || b == null) return a == null && b == null
        if (a.contentEquals(b)) return true
        if (!validGbaRam(a) || !validGbaRam(b)) return false
        val small = if (a.size < b.size) a else b
        val large = if (a.size < b.size) b else a
        for (i in small.indices) if (small[i] != large[i]) return false
        for (i in small.size until large.size) {
            if (large[i] != 0xff.toByte()) return false
        }
        return true
    }

    /** Only change capacity through erased bytes, never truncate progress. */
    internal fun gbaRamAtSize(save: ByteArray, size: Int): ByteArray? {
        if (!validGbaRam(save) || size !in setOf(512, 8192, 32768, 65536, 131072)) {
            return null
        }
        if (save.size > size && (size until save.size).any { save[it] != 0xff.toByte() }) {
            return null
        }
        if (save.size >= size) return save.copyOf(size)
        return ByteArray(size) { 0xff.toByte() }.also { save.copyInto(it) }
    }

    internal enum class SharedGbaSource { NONE, OWN, OTHER, CONFLICT }

    /** Pure migration decision: never infer recency from SAF timestamp or
     * file size, which Syncthing / document providers frequently rewrite.
     */
    internal fun chooseGbaSource(
        own: ByteArray?, other: ByteArray?, lastPublished: ByteArray?
    ): SharedGbaSource = when {
        own == null && other == null -> SharedGbaSource.NONE
        own == null -> SharedGbaSource.OTHER
        other == null -> SharedGbaSource.OWN
        sameGbaRam(own, other) -> SharedGbaSource.OWN
        lastPublished != null && sameGbaRam(own, lastPublished) -> SharedGbaSource.OTHER
        lastPublished != null && sameGbaRam(other, lastPublished) -> SharedGbaSource.OWN
        else -> SharedGbaSource.CONFLICT
    }




    /** Pure preflight: never truncate non-erased bytes or invent a size.
     * Used before any conflict resolution writes and covered by JVM tests.
     */
    internal fun prepareSharedChoice(selected: ByteArray, sizes: List<Int>): List<ByteArray> {
        require(validGbaRam(selected)) { "Selected GBA SRAM size is not supported" }
        require(sizes.isNotEmpty() && sizes.size <= 2) { "Expected one or two core capacities" }
        return sizes.map { size ->
            gbaRamAtSize(selected, size)
                ?: error("Cannot share cartridge SRAM without losing non-erased data")
        }
    }

    /** User-approved conflict arbitration. All reads, hash/size checks and
     * immutable backups happen BEFORE any SRAM mirror is changed. SAF has no
     * multi-document atomic commit, so on a provider failure best-effort
     * rollback is attempted, with original bytes always retained in the
     * private recovery generation.
     *
     * IMPORTANT: no save state is copied between mGBA and gpSP. Only
     * cartridge battery RAM for this exact ROM identity is considered.
     */
    fun resolveSharedGbaConflict(context: Context, target: Target, chosenCore: String) {
        require(target.isGba && target.shareGbaSaves) { "Sharing disabled" }
        require(chosenCore == "mGBA" || chosenCore == "gpSP") { "Invalid core selection" }
        val root = File(context.filesDir, "Saves/SharedGBA")
        val journal = File(root, "${target.romIdentity}.srm")
        val privateFiles = listOf(
            File(context.filesDir, "Saves/mGBA/${target.stem}.srm"),
            File(context.filesDir, "Saves/gpSP/${target.stem}.srm")
        )
        val cores = listOf("mGBA", "gpSP")
        val oldPrivate = privateFiles.map { it.takeIf(File::isFile)?.readBytes() }
        val oldExternal = cores.map { core ->
            target.savesTree?.let { readNamed(context, it, core, "${target.stem}.srm", MAX_SAVE) }
        }
        val authoritative = cores.indices.map { oldExternal[it] ?: oldPrivate[it] }
        val chosenIndex = cores.indexOf(chosenCore)
        require(authoritative.all { it == null || validGbaRam(it) }) {
            "Unsupported SRAM size; kept both original saves"
        }
        val chosen = authoritative[chosenIndex]
            ?: error("No $chosenCore cartridge save to select")
        // Re-run conflict detection at the exact preflight snapshot.
        val prior = journal.takeIf(File::isFile)?.readBytes()
        require(chooseGbaSource(authoritative[target.core.let(cores::indexOf)],
            authoritative[1 - cores.indexOf(target.core)], prior) == SharedGbaSource.CONFLICT) {
            "Saves changed while selecting; reopen to see updated source"
        }
        val nativeSizes = cores.indices.map { index ->
            oldExternal[index]?.size ?: oldPrivate[index]?.size
                ?: if (cores[index] == "gpSP") 131072 else chosen.size
        }
        val converted = prepareSharedChoice(chosen, nativeSizes)
        val canonical = gbaRamAtSize(chosen, 131072)
            ?: error("Selected SRAM contains data beyond supported capacity")

        val backupDir = File(root, "Conflicts/${target.romIdentity}/" +
            "choice-${System.currentTimeMillis()}-${System.nanoTime()}")
        check(backupDir.mkdirs()) { "Cannot allocate immutable SRAM recovery generation" }
        fun record(name: String, data: ByteArray?) {
            val out = File(backupDir, name + if (data == null) ".absent" else ".before")
            stage(out, data ?: byteArrayOf())
        }
        // Save BOTH providers' originals and the previous shared journal
        // before making any changes, even if a core had no prior save.
        for (i in cores.indices) {
            record("${cores[i]}.private", oldPrivate[i])
            record("${cores[i]}.retroarch", oldExternal[i])
        }
        record("shared.journal", prior)
        stage(File(backupDir, "selected.core"), chosenCore.toByteArray(Charsets.UTF_8))
        stage(File(backupDir, "selected.srm"), chosen)
        stage(File(backupDir, "validated"), "preflight complete".toByteArray())

        val changedExternal = mutableListOf<Int>()
        try {
            val external = target.savesTree
            if (external != null) {
                // A different RetroArch/Syncthing writer wins; stop rather
                // than overwriting a change made since the dialog opened.
                for (i in cores.indices) {
                    val observed = readNamed(context, external, cores[i],
                        "${target.stem}.srm", MAX_SAVE)
                    check(sameOptionalBytes(observed, oldExternal[i])) {
                        "RetroArch/${cores[i]} SRAM changed during selection"
                    }
                }
                for (i in cores.indices) {
                    // Even a failed provider write may have truncated the
                    // document before throwing. Track it for rollback.
                    changedExternal.add(i)
                    writeSaf(context, external, cores[i],
                        "${target.stem}.srm", converted[i], isState = false)
                }
            }
            // External writes verified. Now commit private files, including
            // absent destinations; each original already has a backup.
            for (i in cores.indices) stage(privateFiles[i], converted[i])
            stage(journal, canonical)
            stage(File(journal.path + ".core"), chosenCore.toByteArray())
            File(journal.path + ".loaded.srm").delete()
            val conflict = File(journal.path + ".conflict")
            if (conflict.isFile && !conflict.delete()) {
                error("Could not clear SRAM conflict after recovery backup")
            }
            // BOTH cores' old automatic states may point to the discarded
            // timeline. Keep every old byte; launch new SRAM generations.
            for (core in cores) {
                val lower = if (core == "mGBA") "mgba" else "gpsp"
                stage(File(context.filesDir,
                    "Saves/${target.romIdentity}.$lower.skip-resume.once"), byteArrayOf(1))
            }
            stage(File(backupDir, "committed"), "validated".toByteArray())
        } catch (e: Exception) {
            // A partial SAF write can be rolled back only when no external
            // writer changed the value in between. Otherwise retain the
            // immutable recovery generation for explicit repair.
            val external = target.savesTree
            if (external != null) for (i in changedExternal.asReversed()) {
                try {
                    val latest = readNamed(context, external, cores[i],
                        "${target.stem}.srm", MAX_SAVE)
                    if (sameOptionalBytes(latest, converted[i])) {
                        if (oldExternal[i] != null) {
                            writeSaf(context, external, cores[i],
                                "${target.stem}.srm", oldExternal[i]!!, isState = false)
                        } else {
                            // Revert a newly created file only if nobody
                            // else modified it after our attempted export.
                            val folder = coreFolder(context, external, cores[i], false)
                            if (folder != null) {
                                val id = DocumentsContract.getDocumentId(folder)
                                val created = children(context, external, id)
                                    .firstOrNull { it.first == "${target.stem}.srm" }?.second
                                if (created != null) check(DocumentsContract.deleteDocument(
                                    context.contentResolver, created)) {
                                    "Cannot remove newly created SRAM after failed sync"
                                }
                            }
                        }
                    }
                } catch (rollback: Exception) {
                    Log.e(TAG, "SRAM external rollback needs manual recovery", rollback)
                }
            }
            // Restore each modified private mirror iff it is still our
            // chosen image. Another writer's modification is never replaced.
            for (i in cores.indices) try {
                val now = privateFiles[i].takeIf(File::isFile)?.readBytes()
                if (sameOptionalBytes(now, converted[i])) {
                    val old = oldPrivate[i]
                    if (old != null) stage(privateFiles[i], old)
                    else if (privateFiles[i].isFile && !privateFiles[i].delete()) {
                        Log.e(TAG, "Failed to remove newly seeded private SRAM")
                    }
                }
            } catch (restore: Exception) {
                Log.e(TAG, "Private SRAM rollback needs recovery backup", restore)
            }
            // Never delete an immutable recovery snapshot on failure.
            throw SaveConflict("Sharing not changed safely: ${e.message}. Originals backed up.")
        }
    }

    private fun sameOptionalBytes(a: ByteArray?, b: ByteArray?) =
        if (a == null || b == null) a == null && b == null else a.contentEquals(b)

    private fun importSharedGba(context: Context, target: Target) {
        val (journal, loaded) = sharedFiles(context, target)
        val conflict = File(journal.path + ".conflict")
        val previous = journal.takeIf { it.isFile }?.readBytes()
        val ownPrivate = target.saveLocal.takeIf { it.isFile }?.readBytes()
        val peerPrivateFile = otherPrivate(context, target)
        val peerPrivate = peerPrivateFile.takeIf { it.isFile }?.readBytes()
        val ownExternal = target.savesTree?.let {
            readNamed(context, it, target.core, target.stem + ".srm", MAX_SAVE)
        }
        val peerExternal = target.savesTree?.let {
            readNamed(context, it, otherCore(target.core), target.stem + ".srm", MAX_SAVE)
        }
        // External RetroArch saves are authoritative if the user edited
        // either one; private caches only fill genuine external absences.
        val own = ownExternal ?: ownPrivate
        val peer = peerExternal ?: peerPrivate
        val decision = chooseGbaSource(own, peer, previous)
        if (decision == SharedGbaSource.CONFLICT) {
            // The caller MUST ask which source to keep. Do not stage one
            // source over the private or external SRAM while asking: Cancel
            // must leave BOTH cartridges and all their auto-states unchanged.
            stage(conflict, "Unresolved mGBA / gpSP SRAM conflict".toByteArray())
            throw SaveConflict("Different mGBA / gpSP SRAM. Choose a source or cancel.")
        }
        val source = if (decision == SharedGbaSource.OTHER) otherCore(target.core)
            else target.core
        val choice = when (decision) {
            SharedGbaSource.OWN -> own
            SharedGbaSource.OTHER -> peer
            SharedGbaSource.NONE -> previous
            SharedGbaSource.CONFLICT -> null
        }
        if (choice == null) {
            loaded.delete()
            return
        }
        if (!validGbaRam(choice) || (previous != null && !validGbaRam(previous))) {
            stage(conflict, "Unsupported GBA SRAM size; no shared writes".toByteArray())
            return
        }
        conflict.delete()
        if (ownPrivate != null && !ownPrivate.contentEquals(choice)) {
            // Preserve an old private slot cache before importing a newer
            // SRAM from the other core or from RetroArch's SAF folder.
            val backup = File(journal.path + ".${target.core}.before-share.srm")
            if (!backup.exists()) stage(backup, ownPrivate)
        }
        stage(target.saveLocal, choice)
        // Never overwrite the OTHER core's existing private save before the
        // launched core confirms it can deserialize the shared battery RAM.
        stage(loaded, choice)
        val lastWriter = File(journal.path + ".core").takeIf { it.isFile }?.readText()?.trim()
        if (source != target.core || (lastWriter != null && lastWriter != target.core)) {
            // A stale per-core .state.auto must NEVER roll back a newer SRAM
            // imported from the other emulator.
            val lower = if (target.core == "mGBA") "mgba" else "gpsp"
            val marker = File(context.filesDir, "Saves/${target.romIdentity}.$lower.skip-resume.once")
            stage(marker, byteArrayOf(1))
        }
    }

    private fun exportSharedGba(context: Context, target: Target) {
        val current = target.saveLocal.takeIf { it.isFile }?.readBytes() ?: return
        if (!validGbaRam(current)) error("unsupported GBA SRAM size, keeping existing mirrors")
        val (journal, loadedFile) = sharedFiles(context, target)
        if (File(journal.path + ".conflict").exists()) {
            // The currently selected core can keep its own progress; do not
            // destroy the other emulator's differing SRAM.
            target.savesTree?.let {
                writeSaf(context, it, target.core, target.stem + ".srm",
                    current, isState = false)
            }
            error("mGBA and gpSP saves conflict; both folders kept separate")
        }
        val baseline = journal.takeIf { it.isFile }?.readBytes()
        val atLaunch = loadedFile.takeIf { it.isFile }?.readBytes()
        if (baseline != null && (gbaRamAtSize(current, baseline.size) == null ||
                    gbaRamAtSize(baseline, current.size) == null)) {
            Log.w(TAG, "Shared GBA mismatch: core=${target.core} " +
                "current=${current.size} published=${baseline.size} " +
                "loaded=${atLaunch?.size ?: 0}, non-erased bytes prevent safe capacity conversion")
            error("GBA save sizes differ with non-erased data; no mirrors overwritten")
        }

        // Construct all file payloads before touching either core's mirrors.
        // A 128 KiB gpSP buffer may safely contain a 512B/8K/32K/64K mGBA
        // save followed ONLY by 0xFF. Never truncate other bytes.
        val otherLocal = otherPrivate(context, target)
        val otherOld = otherLocal.takeIf { it.isFile }?.readBytes()
        if (otherOld != null && !validGbaRam(otherOld)) {
            error("Other core private save has an unsupported SRAM size; no mirrors overwritten")
        }
        if (otherOld != null && !sameGbaRam(otherOld, current) &&
            !sameGbaRam(otherOld, baseline) && !sameGbaRam(otherOld, atLaunch)) {
            error("Other core private SRAM changed independently; no mirrors overwritten")
        }
        val peerSize = otherOld?.size ?: if (otherCore(target.core) == "gpSP") 131072 else current.size
        val peerData = gbaRamAtSize(current, peerSize)
            ?: error("Other core has a different active SRAM capacity; no mirrors overwritten")
        val canonical = gbaRamAtSize(current, 131072)
            ?: error("Cannot safely represent shared GBA SRAM; no mirrors overwritten")

        val external = target.savesTree
        val writes = mutableListOf<Pair<String, ByteArray>>()
        if (external != null) {
            // PRE-FLIGHT both destinations before writing ANY of them. A
            // concurrent RetroArch/Syncthing edit is a conflict, not a signal
            // to overwrite the other core's progress.
            for (core in arrayOf(target.core, otherCore(target.core))) {
                val old = readNamed(context, external, core, target.stem + ".srm", MAX_SAVE)
                if (old != null && !validGbaRam(old)) {
                    error("$core SRAM has an unrecognized size; mirroring cancelled")
                }
                if (old != null && !sameGbaRam(old, current) &&
                    !sameGbaRam(baseline, old) && !sameGbaRam(atLaunch, old)) {
                    error("$core SRAM changed outside Slot; mirroring cancelled")
                }
                val destSize = old?.size ?: if (core == target.core) current.size
                    else peerSize
                val data = gbaRamAtSize(current, destSize)
                    ?: error("$core SRAM has non-erased data beyond its capacity; mirroring cancelled")
                writes.add(core to data)
            }
        }
        Log.i(TAG, "Shared GBA export: core=${target.core}, native=${current.size}, " +
            "published=${baseline?.size ?: 0}, loaded=${atLaunch?.size ?: 0}, " +
            "peer=${otherOld?.size ?: 0}, erased-padding-only=true")
        if (external != null) {
            for ((core, data) in writes) {
                writeSaf(context, external, core, target.stem + ".srm",
                    data, isState = false)
            }
        }
        if (otherOld != null && !otherOld.contentEquals(peerData)) {
            val backup = File(journal.path + ".${otherCore(target.core)}.before-share.srm")
            if (!backup.exists()) stage(backup, otherOld)
        }
        stage(otherLocal, peerData)
        // Canonical journal is always 128 KiB; private and external per-core
        // files retain their actual capacity whenever a file already exists.
        stage(journal, canonical)
        stage(File(journal.path + ".core"), target.core.toByteArray())
        loadedFile.delete()
    }

    /** Export each new Slot Polaroid to the first EMPTY RetroArch manual slot.
     * Never replace the user's existing .state/.state1..state9 files.
     * The private 10-entry Polaroid ring remains independent.
     */
    fun exportManualState(context: Context, target: Target, stamp: String): List<String> {
        val failures = mutableListOf<String>()
        try {
            require(stamp.matches(Regex("[0-9]{4}-[0-9]{2}-[0-9]{2}_[0-9]{2}-[0-9]{2}-[0-9]{2}"))) {
                "Invalid Slot state timestamp"
            }
            val staged = File(target.manualExportPrefix.absolutePath + "." + stamp + ".rastate")
            val bytes = staged.readBytes()
            RetroArchCompression.requireSupportedContainer(bytes)
            val compressed = RetroArchCompression.encode(bytes)
            for (slot in 0..9) {
                val name = target.stem + if (slot == 0) ".state" else ".state$slot"
                if (target.statesTree != null) {
                    if (find(context, target.statesTree, target.core, name) != null) continue
                    writeSaf(context, target.statesTree, target.core,
                        name, compressed, isState = true, createOnly = true)
                } else {
                    val local = File(target.stateDefaultLocal.parentFile, name)
                    if (local.exists()) continue
                    stage(local, compressed)
                    check(local.readBytes().contentEquals(compressed)) {
                        "Private numbered state verification failed"
                    }
                }
                if (!staged.delete()) Log.w(TAG, "Manual staging cleanup deferred: $staged")
                return failures
            }
            error("Manual slots 0-9 are occupied; Polaroid remains saved inside Slot")
        } catch (e: Exception) {
            Log.e(TAG, "RetroArch manual state export skipped", e)
            failures.add("Manual state sync skipped: " + (e.message ?: "unsupported format"))
        }
        return failures
    }

    private fun queryRomName(context: Context, uri: Uri): String {
        context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME),
            null, null, null)?.use { cursor ->
            if (cursor.moveToFirst()) {
                val n = cursor.getString(0)
                if (!n.isNullOrBlank()) return n
            }
        }
        error("Cannot read ROM filename from Android document provider")
    }

    private fun ByteArray.startsWithRASTATE(): Boolean =
        size >= 8 && copyOfRange(0, 7).contentEquals("RASTATE".toByteArray()) && this[7] == 1.toByte()

    private fun children(context: Context, root: Uri, parent: String): List<Pair<String, Uri>> {
        val uri = DocumentsContract.buildChildDocumentsUriUsingTree(root, parent)
        val names = mutableListOf<Pair<String, Uri>>()
        val columns = arrayOf(
            DocumentsContract.Document.COLUMN_DOCUMENT_ID,
            DocumentsContract.Document.COLUMN_DISPLAY_NAME
        )
        context.contentResolver.query(uri, columns, null, null, null)?.use { c ->
            val idCol = c.getColumnIndexOrThrow(columns[0])
            val nameCol = c.getColumnIndexOrThrow(columns[1])
            while (c.moveToNext()) {
                val id = c.getString(idCol) ?: continue
                val name = c.getString(nameCol) ?: continue
                names.add(name to DocumentsContract.buildDocumentUriUsingTree(root, id))
            }
        } ?: error("Cannot enumerate selected RetroArch folder")
        return names
    }

    private fun coreFolder(context: Context, root: Uri, core: String, create: Boolean): Uri? {
        val rootId = DocumentsContract.getTreeDocumentId(root)
        val existing = children(context, root, rootId).firstOrNull { it.first == core }?.second
        if (existing != null) return existing
        if (!create) return null
        // ACTION_OPEN_DOCUMENT_TREE returns a tree URI without a /document/
        // segment. createDocument() requires a document URI, even when that
        // document is the selected tree root. Supplying `root` directly causes
        // ExternalStorageProvider to reject creation with "Invalid URI".
        val rootDocument = DocumentsContract.buildDocumentUriUsingTree(root, rootId)
        return DocumentsContract.createDocument(context.contentResolver, rootDocument,
            DocumentsContract.Document.MIME_TYPE_DIR, core)
            ?: error("Cannot create RetroArch core folder: $core")
    }

    /**
     * The AOSP external-storage document provider uses stable hierarchical IDs
     * such as "primary:RetroArch/states/mGBA/Game.state.auto". A tree grant
     * covers its descendants; we do not infer a file-system path or bypass SAF.
     * Never use this optimisation for downloads/cloud/third-party providers,
     * whose document identifiers need not be hierarchical.
     */
    internal fun directDocumentId(treeId: String, core: String, name: String): String? {
        if (core != "mGBA" && core != "gpSP") return null
        if (name.isEmpty() || name == "." || name == ".." ||
            name.any { it == '/' || it == '\\' || it.code < 32 }) return null
        val volume = treeId.substringBefore(':', "")
        if (volume != "primary" && !volume.matches(Regex("[0-9a-fA-F]{4}-[0-9a-fA-F]{4}"))) {
            return null
        }
        if (treeId.any { it == '\\' || it.code < 32 } ||
            treeId.split('/').any { it == "." || it == ".." }) return null
        return "$treeId/$core/$name"
    }

    private fun directDocument(root: Uri, core: String, name: String): Uri? {
        if (root.authority != "com.android.externalstorage.documents") return null
        val treeId = try {
            DocumentsContract.getTreeDocumentId(root)
        } catch (_: IllegalArgumentException) {
            return null
        }
        val childId = directDocumentId(treeId, core, name) ?: return null
        // Only the selected SAF tree, not a raw filesystem path, is accessed.
        return DocumentsContract.buildDocumentUriUsingTree(root, childId)
    }

    /**
     * ExternalStorageProvider can wrap a *missing* child file in an
     * IllegalArgumentException during its tree ancestry check, rather than
     * throwing FileNotFoundException from openInputStream().
     *
     * Only tolerate the precise "Missing file for <requested document ID>"
     * condition. Permission errors, malformed URIs, provider failures, and
     * missing *other* paths must still surface to the caller.
     */
    internal fun isMissingDirectDocument(error: IllegalArgumentException, requestedId: String): Boolean {
        var current: Throwable? = error
        repeat(8) {
            val issue = current ?: return false
            if (issue is FileNotFoundException) {
                return issue.message?.contains("Missing file for $requestedId") == true
            }
            val message = issue.message.orEmpty()
            if (message.startsWith("Failed to determine if ") &&
                message.contains("java.io.FileNotFoundException: Missing file for $requestedId")) {
                return true
            }
            current = issue.cause
        }
        return false
    }

    private fun readNamed(context: Context, root: Uri, core: String,
                          name: String, limit: Int): ByteArray? {
        val direct = directDocument(root, core, name)
        if (direct != null) {
            return try {
                read(context, direct, limit)
            } catch (_: FileNotFoundException) {
                // An absent SRAM, RTC or auto-state is optional.
                null
            } catch (error: IllegalArgumentException) {
                val requestedId = DocumentsContract.getDocumentId(direct)
                if (!isMissingDirectDocument(error, requestedId)) throw error
                // Android's ExternalStorageProvider rejected the missing child
                // during its isChildDocument() check. Continue importing the
                // *other* files, without an expensive fallback directory scan.
                Log.d(TAG, "Optional RetroArch file absent: $core/$name")
                null
            }
        }
        return find(context, root, core, name)?.let { read(context, it, limit) }
    }

    private fun find(context: Context, root: Uri, core: String, name: String): Uri? {
        val dir = coreFolder(context, root, core, create = false) ?: return null
        val id = DocumentsContract.getDocumentId(dir)
        return children(context, root, id).firstOrNull { it.first == name }?.second
    }

    private fun read(context: Context, uri: Uri, limit: Int): ByteArray =
        context.contentResolver.openInputStream(uri)?.use { input ->
            val out = java.io.ByteArrayOutputStream()
            val buffer = ByteArray(65536)
            while (true) {
                val len = input.read(buffer)
                if (len < 0) break
                out.write(buffer, 0, len)
                require(out.size() <= limit) { "RetroArch file exceeds safety limit" }
            }
            out.toByteArray()
        } ?: error("Cannot read RetroArch file")

    internal fun stage(local: File, bytes: ByteArray) {
        // Opening the same game must not rewrite and fsync an unchanged SRAM
        // or automatic state every time. Read+compare is cheap for these
        // bounded files and preserves the exact externally supplied bytes.
        if (local.isFile && local.length() == bytes.size.toLong()) {
            try {
                if (local.readBytes().contentEquals(bytes)) return
            } catch (_: java.io.IOException) {
                // Existing file cannot be read; replace it atomically below.
            }
        }
        // An untouched installation may have only the currently used core's
        // Saves directory. Create the other core's path before the SRAM mirror.
        val parent = requireNotNull(local.parentFile) { "Save has no parent directory" }
        check(parent.isDirectory || parent.mkdirs()) { "Cannot create save directory: $parent" }
        check(parent.isDirectory) { "Save path is not a directory: $parent" }
        val pending = File(local.parentFile, local.name + ".partial")
        try {
            pending.outputStream().use { it.write(bytes); it.fd.sync() }
            check(pending.renameTo(local)) { "Cannot import RetroArch file" }
        } finally {
            pending.delete()
        }
    }

    private fun writeSaf(context: Context, root: Uri, core: String,
                         name: String, bytes: ByteArray, isState: Boolean,
                         createOnly: Boolean = false) {
        val resolver = context.contentResolver
        val folder = coreFolder(context, root, core, create = true)
            ?: error("Cannot access RetroArch core folder")
        val dirId = DocumentsContract.getDocumentId(folder)
        val existing = children(context, root, dirId).firstOrNull { it.first == name }?.second
        require(!createOnly || existing == null) {
            "Manual RetroArch slot was occupied during sync; preserved"
        }
        if (existing != null) {
            val old = read(context, existing, if (isState) MAX_STATE else MAX_SAVE)
            if (old.contentEquals(bytes)) return
            if (isState) {
                // Never overwrite unsupported or damaged RetroArch states,
                // including zstd v2 and legacy raw files we cannot validate.
                RetroArchCompression.requireSupportedContainer(old)
            }
            // Never destroy the pre-Slot save on first sync. A backup is
            // retained in a dedicated subdirectory of the same core folder.
            val backupName = name + ".before-slot"
            val backups = children(context, root, dirId)
            if (backups.none { it.first == backupName }) {
                val backup = DocumentsContract.createDocument(resolver, folder,
                    "application/octet-stream", backupName)
                    ?: error("Cannot back up existing RetroArch file")
                resolver.openOutputStream(backup, "wt")?.use { it.write(old) }
                    ?: error("Cannot write RetroArch backup")
                check(read(context, backup, if (isState) MAX_STATE else MAX_SAVE)
                    .contentEquals(old)) {
                    "RetroArch backup verification failed; external state preserved"
                }
            }
            // Best-effort conflict guard: abort if a sync service or RetroArch
            // modified the source while we were preparing the replacement.
            check(read(context, existing, if (isState) MAX_STATE else MAX_SAVE)
                .contentEquals(old)) {
                "RetroArch file changed during sync; refusing to overwrite"
            }
        }
        val target = existing ?: DocumentsContract.createDocument(resolver, folder,
            "application/octet-stream", name)
            ?: error("Cannot create RetroArch save")
        if (createOnly) {
            // Some SAF providers silently rename a duplicate filename. Never
            // misreport that as an available RetroArch slot.
            val actual = resolver.query(target, arrayOf(OpenableColumns.DISPLAY_NAME),
                null, null, null)?.use { cursor ->
                if (cursor.moveToFirst()) cursor.getString(0) else null
            }
            if (actual != name) {
                DocumentsContract.deleteDocument(resolver, target)
                error("Manual state slot name collision; external file preserved")
            }
        }
        // Android SAF generally has no cross-provider atomic rename guarantee.
        // Write with truncate, and verify contents. The untouched .before-slot
        // copy remains recoverable if a provider fails mid-write.
        resolver.openOutputStream(target, "wt")?.use { it.write(bytes); it.flush() }
            ?: error("Cannot write RetroArch save")
        check(read(context, target, if (isState) MAX_STATE else MAX_SAVE).contentEquals(bytes)) {
            "RetroArch sync verification failed; backup is preserved"
        }
    }
}
