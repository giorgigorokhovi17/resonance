package app.resonance.player

import android.Manifest
import android.content.ComponentName
import android.content.Intent
import android.content.pm.PackageManager
import android.database.ContentObserver
import android.graphics.Color
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.DocumentsContract
import android.provider.MediaStore
import android.provider.OpenableColumns
import android.view.View
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.ListenableFuture
import java.io.File
import java.lang.ref.WeakReference

class MainActivity : TauriActivity() {
  private var controller: ListenableFuture<MediaController>? = null
  private val handler = Handler(Looper.getMainLooper())

  // Music folders (docs/android-design.md §4): the app reads audio by path with READ_MEDIA_AUDIO;
  // the system folder picker only chooses which folders.
  private val askAudio = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
    if (granted) openPicker()
    else NativeBridge.answer(null, "Undertone needs access to music and audio to read your songs. Allow it in Settings → Apps → Undertone → Permissions.")
  }
  private val pickTree = registerForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
    if (uri == null) {
      NativeBridge.answer(null, null)
      return@registerForActivityResult
    }
    val path = Storage.treePath(uri)
    if (path == null) {
      NativeBridge.answer(null, "Pick a folder on the phone's storage or SD card.")
    } else {
      // Kept for reading lyrics and folder pictures, which the audio permission doesn't cover.
      runCatching { contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION) }
      NativeBridge.answer(path, null)
    }
  }

  private var pickMultiple = false
  private val pickDocs = registerForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
    val chosen = if (pickMultiple) uris else uris.take(1)
    if (chosen.isEmpty()) {
      NativeBridge.answer(null, null)
      return@registerForActivityResult
    }
    Thread {
      runCatching {
        val root = File(cacheDir, "picked")
        root.deleteRecursively()
        val dir = File(root, System.currentTimeMillis().toString()).apply { mkdirs() }
        chosen.mapIndexed { idx, uri ->
          val rawName = displayName(uri) ?: "file-$idx"
          val safe = rawName.replace(Regex("[/\\\\\\x00-\\x1f]"), "_").ifEmpty { "file-$idx" }
          val dst = File(dir, safe)
          contentResolver.openInputStream(uri)?.use { input ->
            dst.outputStream().use { out -> input.copyTo(out) }
          } ?: error("Can't read \"$safe\".")
          dst.absolutePath
        }.joinToString("\n")
      }.onSuccess { paths ->
        NativeBridge.answer(paths, null)
      }.onFailure { e ->
        NativeBridge.answer(null, e.message ?: "Couldn't read the selected file.")
      }
    }.start()
  }

  private var pendingExportSrc: String? = null
  private val createDoc = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { res ->
    val uri = if (res.resultCode == RESULT_OK) res.data?.data else null
    val src = pendingExportSrc
    pendingExportSrc = null
    if (uri == null || src == null) {
      NativeBridge.answer(null, null)
      return@registerForActivityResult
    }
    Thread {
      runCatching {
        File(src).inputStream().use { input ->
          contentResolver.openOutputStream(uri, "wt")?.use { out -> input.copyTo(out) }
            ?: error("Can't write to the chosen file.")
        }
      }.onSuccess {
        NativeBridge.answer("ok", null)
      }.onFailure { e ->
        NativeBridge.answer(null, e.message ?: "Couldn't save the file.")
      }
    }.start()
  }

  /** Songs added or removed by other apps (downloads, file managers) while the app is open. */
  private val mediaChanged = object : ContentObserver(handler) {
    private val rescan = Runnable { NativeBridge.libraryChanged() }
    override fun onChange(selfChange: Boolean) {
      handler.removeCallbacks(rescan)
      handler.postDelayed(rescan, 2000)
    }
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    // Light icons on the app's dark background (the default theme's --c-bg, src/lib/theme.ts).
    enableEdgeToEdge(
      statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
      navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
    )
    super.onCreate(savedInstanceState)
    window.decorView.setBackgroundColor(Color.parseColor("#07080b"))
    // Android 15 draws apps under the status and navigation bars; keep the page clear of them.
    val content = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
      view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
      WindowInsetsCompat.CONSUMED
    }
    NativeBridge.appContext = applicationContext
    NativeBridge.activity = WeakReference(this)
    NativeBridge.activityCreated()
  }

  override fun onStart() {
    super.onStart()
    // Binding starts the playback service; it turns itself into a foreground service while
    // music plays, so playback outlives this screen.
    val token = SessionToken(this, ComponentName(this, PlaybackService::class.java))
    controller = MediaController.Builder(this, token).buildAsync()
    contentResolver.registerContentObserver(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI, true, mediaChanged)
  }

  override fun onResume() {
    super.onResume()
    NativeBridge.libraryChanged()
  }

  override fun onStop() {
    contentResolver.unregisterContentObserver(mediaChanged)
    controller?.let { MediaController.releaseFuture(it) }
    controller = null
    super.onStop()
  }

  fun pickMusicFolder() {
    if (ContextCompat.checkSelfPermission(this, Manifest.permission.READ_MEDIA_AUDIO) == PackageManager.PERMISSION_GRANTED) {
      openPicker()
    } else {
      askAudio.launch(Manifest.permission.READ_MEDIA_AUDIO)
    }
  }

  private fun openPicker() {
    // Start in Music, where most phones keep songs.
    val music = DocumentsContract.buildDocumentUri("com.android.externalstorage.documents", "primary:Music")
    pickTree.launch(music)
  }

  fun pickFiles(multiple: Boolean) {
    pickMultiple = multiple
    pickDocs.launch(arrayOf("*/*"))
  }

  fun exportFile(src: String, name: String) {
    pendingExportSrc = src
    val ext = name.substringAfterLast('.', "").lowercase()
    val mime = when (ext) {
      "m3u", "m3u8" -> "audio/x-mpegurl"
      "xspf" -> "application/xspf+xml"
      "csv" -> "text/csv"
      "json", "resonance-backup" -> "application/json"
      "txt" -> "text/plain"
      else -> "application/octet-stream"
    }
    val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
      addCategory(Intent.CATEGORY_OPENABLE)
      type = mime
      putExtra(Intent.EXTRA_TITLE, name)
    }
    createDoc.launch(intent)
  }

  private fun displayName(uri: android.net.Uri): String? {
    contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
      if (c.moveToFirst()) {
        val i = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
        if (i >= 0 && !c.isNull(i)) return c.getString(i)
      }
    }
    return uri.lastPathSegment?.substringAfterLast('/')
  }
}
