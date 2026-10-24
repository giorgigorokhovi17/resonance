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
import java.lang.ref.WeakReference

class MainActivity : TauriActivity() {
  private var controller: ListenableFuture<MediaController>? = null
  private val handler = Handler(Looper.getMainLooper())

  // Music folders (docs/android-design.md §4): the app reads audio by path with READ_MEDIA_AUDIO;
  // the system folder picker only chooses which folders.
  private val askAudio = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
    if (granted) openPicker()
    else NativeBridge.folderPicked(null, "Undertone needs access to music and audio to read your songs. Allow it in Settings → Apps → Undertone → Permissions.")
  }
  private val pickTree = registerForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
    if (uri == null) {
      NativeBridge.folderPicked(null, null)
      return@registerForActivityResult
    }
    val path = Storage.treePath(uri)
    if (path == null) {
      NativeBridge.folderPicked(null, "Pick a folder on the phone's storage or SD card.")
    } else {
      // Kept for reading lyrics and folder pictures, which the audio permission doesn't cover.
      runCatching { contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION) }
      NativeBridge.folderPicked(path, null)
    }
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
}
