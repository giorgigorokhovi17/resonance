package app.resonance.player

import android.os.Handler
import android.os.Looper
import android.os.SystemClock

/**
 * The Rust audio engine, reached through JNI (src-tauri/src/mobile/smtc.rs). Rust pushes the
 * current song and play state here; the media session reads them and sends commands back.
 */
object NativeBridge {
  init {
    System.loadLibrary("resonance_lib")
  }

  const val STOPPED = 0
  const val PLAYING = 1
  const val PAUSED = 2

  /** Lets Rust find this class from its own threads, and resends the current state. */
  @JvmStatic external fun register()

  /** MainActivity created; rebuilds the page if the previous screen took it along. */
  @JvmStatic external fun activityCreated()

  /**
   * What the user did on a system screen opened for Rust (folder picker, file picker, save as):
   * a value, null when cancelled, or an error to show (src/mobile/library.rs).
   */
  @JvmStatic external fun answer(value: String?, error: String?)

  /** Back on screen, or Android's media index changed: rescan the library. */
  @JvmStatic external fun libraryChanged()

  /** Whether MainActivity is visible; Soulseek signs in only while visible or downloading. */
  @JvmStatic external fun setAppVisible(visible: Boolean)

  /** Whether SoulseekJobService is currently running a user-initiated download job. */
  @JvmStatic external fun setDownloadJobRunning(running: Boolean)

  /** The screen that can show the folder picker. */
  @Volatile var activity: java.lang.ref.WeakReference<MainActivity>? = null

  /** Set by MainActivity and PlaybackService, whichever starts first. */
  @Volatile var appContext: android.content.Context? = null

  /** Called by Rust while scanning: a folder's picture the file system hides (Storage.kt). */
  @JvmStatic
  fun folderImage(dir: String): ByteArray? =
    appContext?.let { runCatching { Storage.folderImage(it, dir) }.getOrNull() }

  /** Called by Rust: loads saved Soulseek credentials ("username\npassword") from Android Keystore. */
  @JvmStatic
  fun loadCredentials(): String? =
    appContext?.let { Soulseek.loadCredentials(it) }

  /** Called by Rust: encrypts and saves Soulseek credentials in Android Keystore (null = ok, or error). */
  @JvmStatic
  fun saveCredentials(username: String, password: String): String? =
    appContext?.let { Soulseek.saveCredentials(it, username, password) }
      ?: "The app isn't ready yet."

  /** Called by Rust: removes saved Soulseek credentials from Android Keystore. */
  @JvmStatic
  fun deleteCredentials() {
    appContext?.let { Soulseek.deleteCredentials(it) }
  }

  /** Called by Rust: app-specific external storage directory for in-progress Soulseek downloads. */
  @JvmStatic
  fun soulseekStagingDir(): String? =
    appContext?.let { runCatching { Soulseek.stagingDir(it) }.getOrNull() }

  /** Called by Rust: publishes a finished Soulseek download into Music/Undertone/Soulseek via MediaStore. */
  @JvmStatic
  fun publishSoulseekDownload(srcPath: String, subfolder: String?, name: String): String? =
    appContext?.let { Soulseek.publishDownload(it, srcPath, subfolder, name) }

  /** Called by Rust when the Soulseek download queue or network preference changes. */
  @JvmStatic
  fun onSoulseekQueue(activeCount: Int, wifiOnly: Boolean) {
    val ctx = appContext ?: return
    main.post { SoulseekJobService.onQueueChanged(ctx, activeCount, wifiOnly) }
  }

  /** Called by Rust: asks for the audio permission if needed, then opens the folder picker. */
  @JvmStatic
  fun pickMusicFolder() = onScreen { it.pickMusicFolder() }

  /** Called by Rust: the system file picker; answers with the paths of copies in the cache. */
  @JvmStatic
  fun pickFiles(multiple: Boolean) = onScreen { it.pickFiles(multiple) }

  /** Called by Rust: the system "save as" screen for `name`, then copies `src` there. */
  @JvmStatic
  fun exportFile(src: String, name: String) = onScreen { it.exportFile(src, name) }

  private fun onScreen(open: (MainActivity) -> Unit) {
    main.post {
      val screen = activity?.get()
      if (screen == null) answer(null, "Open Undertone first.") else open(screen)
    }
  }

  /** "play", "pause", "toggle", "next", "previous", "stop", or "seek" to `value` seconds. */
  @JvmStatic external fun command(name: String, value: Double)

  fun command(name: String) = command(name, 0.0)

  class Track(val title: String, val artist: String, val album: String, val cover: String?, val durationMs: Long)

  @Volatile var track: Track? = null
    private set
  @Volatile var state = STOPPED
    private set
  /** Position at `positionAt` (elapsedRealtime), so it can be extrapolated while playing. */
  @Volatile var positionMs = 0L
    private set
  @Volatile var positionAt = 0L
    private set

  /** Called on the main thread after every change. */
  var listener: (() -> Unit)? = null

  private val main = Handler(Looper.getMainLooper())

  @JvmStatic
  fun onTrack(title: String, artist: String, album: String, cover: String?, duration: Double) {
    track = Track(title, artist, album, cover, (duration * 1000).toLong())
    main.post { listener?.invoke() }
  }

  @JvmStatic
  fun onPlayback(newState: Int, position: Double) {
    state = newState
    positionMs = (position * 1000).toLong()
    positionAt = SystemClock.elapsedRealtime()
    main.post { listener?.invoke() }
  }
}
