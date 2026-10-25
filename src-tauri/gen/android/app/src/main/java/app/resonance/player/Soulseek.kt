package app.resonance.player

import android.content.ContentValues
import android.content.Context
import android.os.Environment
import android.provider.MediaStore
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Soulseek on Android (docs/android-design.md §5):
 * - Sign-in credentials encrypted with an Android Keystore AES-256-GCM key into an app-private file.
 * - Downloads stage in app-specific external storage (`getExternalFilesDir("soulseek")`), then on
 *   completion publish into `Music/Undertone/Soulseek` through MediaStore (`IS_PENDING`).
 */
object Soulseek {
  private const val KEYSTORE = "AndroidKeyStore"
  private const val KEY_ALIAS = "soulseek_credentials"
  private const val CRED_FILE = "soulseek.cred"
  private const val GCM_TAG_BITS = 128

  fun loadCredentials(context: Context): String? = runCatching {
    val file = File(context.filesDir, CRED_FILE)
    if (!file.isFile) return null
    val bytes = file.readBytes()
    if (bytes.size < 2) return null
    val ivLen = bytes[0].toInt() and 0xff
    if (ivLen <= 0 || bytes.size <= 1 + ivLen) return null
    val keyStore = KeyStore.getInstance(KEYSTORE).apply { load(null) }
    val key = keyStore.getKey(KEY_ALIAS, null) as? SecretKey ?: return null
    val iv = bytes.copyOfRange(1, 1 + ivLen)
    val ct = bytes.copyOfRange(1 + ivLen, bytes.size)
    val cipher = Cipher.getInstance("AES/GCM/NoPadding")
    cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(GCM_TAG_BITS, iv))
    String(cipher.doFinal(ct), Charsets.UTF_8)
  }.getOrNull()

  fun saveCredentials(context: Context, username: String, password: String): String? = runCatching {
    val key = getOrCreateKey()
    val cipher = Cipher.getInstance("AES/GCM/NoPadding")
    cipher.init(Cipher.ENCRYPT_MODE, key)
    val iv = cipher.iv
    val ct = cipher.doFinal("$username\n$password".toByteArray(Charsets.UTF_8))
    val out = ByteArray(1 + iv.size + ct.size)
    out[0] = iv.size.toByte()
    System.arraycopy(iv, 0, out, 1, iv.size)
    System.arraycopy(ct, 0, out, 1 + iv.size, ct.size)
    val tmp = File(context.filesDir, "$CRED_FILE.tmp")
    val dst = File(context.filesDir, CRED_FILE)
    tmp.writeBytes(out)
    if (!tmp.renameTo(dst)) {
      dst.writeBytes(out)
      tmp.delete()
    }
  }.exceptionOrNull()?.let { e ->
    "Could not save the sign-in to the Android Keystore: ${e.message ?: e.javaClass.simpleName}"
  }

  fun deleteCredentials(context: Context) {
    runCatching { File(context.filesDir, CRED_FILE).delete() }
    runCatching {
      val ks = KeyStore.getInstance(KEYSTORE).apply { load(null) }
      if (ks.containsAlias(KEY_ALIAS)) ks.deleteEntry(KEY_ALIAS)
    }
  }

  private fun getOrCreateKey(): SecretKey {
    val ks = KeyStore.getInstance(KEYSTORE).apply { load(null) }
    (ks.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
    val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
    val spec = KeyGenParameterSpec.Builder(
      KEY_ALIAS,
      KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
    )
      .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
      .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
      .setKeySize(256)
      .build()
    gen.init(spec)
    return gen.generateKey()
  }

  /** App-specific external storage where in-progress downloads are written. */
  fun stagingDir(context: Context): String {
    val base = context.getExternalFilesDir(null) ?: context.filesDir
    return File(base, "soulseek").apply { mkdirs() }.absolutePath
  }

  /**
   * Publishes a completed download from app-specific staging (`srcPath`) into
   * `Music/Undertone/Soulseek[/subfolder]/name` through MediaStore (`IS_PENDING`).
   * Never overwrites an existing finished file.
   */
  fun publishDownload(context: Context, srcPath: String, subfolder: String?, name: String): String {
    return runCatching {
      val src = File(srcPath)
      if (!src.isFile) error("Downloaded file is missing.")
      val cleanSub = subfolder?.trim('/')?.takeIf { it.isNotEmpty() }
      val relDir = if (cleanSub != null) "Music/Undertone/Soulseek/$cleanSub" else "Music/Undertone/Soulseek"
      val musicRoot = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_MUSIC)
      val finalDir = if (cleanSub != null) File(musicRoot, "Undertone/Soulseek/$cleanSub") else File(musicRoot, "Undertone/Soulseek")
      val finalFile = File(finalDir, name)
      if (finalFile.exists()) {
        src.delete()
        error("A file with this name is already in the download folder.")
      }

      val resolver = context.contentResolver
      val values = ContentValues().apply {
        put(MediaStore.MediaColumns.DISPLAY_NAME, name)
        put(MediaStore.MediaColumns.MIME_TYPE, audioMime(name))
        put(MediaStore.MediaColumns.RELATIVE_PATH, "$relDir/")
        put(MediaStore.MediaColumns.IS_PENDING, 1)
      }
      val uri = resolver.insert(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI, values)
        ?: error("MediaStore refused the new audio file.")
      try {
        src.inputStream().use { input ->
          resolver.openOutputStream(uri)?.use { out -> input.copyTo(out) }
            ?: error("Could not open MediaStore output stream.")
        }
        val done = ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }
        resolver.update(uri, done, null, null)
      } catch (e: Throwable) {
        runCatching { resolver.delete(uri, null, null) }
        throw e
      }
      src.delete()

      var resolvedPath: String? = null
      resolver.query(uri, arrayOf(MediaStore.MediaColumns.DATA), null, null, null)?.use { c ->
        if (c.moveToFirst()) {
          val i = c.getColumnIndex(MediaStore.MediaColumns.DATA)
          if (i >= 0 && !c.isNull(i)) resolvedPath = c.getString(i)
        }
      }
      resolvedPath ?: finalFile.absolutePath
    }.fold(
      onSuccess = { it },
      onFailure = { e -> "ERR:${e.message ?: "Could not publish download."}" },
    )
  }

  private fun audioMime(name: String): String = when (name.substringAfterLast('.', "").lowercase()) {
    "flac" -> "audio/flac"
    "mp3" -> "audio/mpeg"
    "m4a", "aac" -> "audio/mp4"
    "ogg", "oga" -> "audio/ogg"
    "opus" -> "audio/opus"
    "wav" -> "audio/x-wav"
    "aiff", "aif" -> "audio/x-aiff"
    else -> "audio/*"
  }
}
