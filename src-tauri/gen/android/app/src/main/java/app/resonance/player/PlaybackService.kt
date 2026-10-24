package app.resonance.player

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.net.Uri
import android.os.Looper
import android.os.SystemClock
import androidx.annotation.OptIn
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import java.io.File

/**
 * Keeps playback going with the screen off and the app swiped away, and gives Android the media
 * notification, lock-screen and Bluetooth controls (docs/android-design.md §2). The sound itself
 * comes from the Rust engine; this only mirrors its state and forwards commands.
 */
class PlaybackService : MediaSessionService() {
  private var session: MediaSession? = null

  override fun onCreate() {
    super.onCreate()
    NativeBridge.appContext = applicationContext
    val open = PendingIntent.getActivity(
      this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
    )
    session = MediaSession.Builder(this, RustPlayer(this)).setSessionActivity(open).build()
  }

  override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = session

  override fun onDestroy() {
    session?.run {
      player.release()
      release()
    }
    session = null
    super.onDestroy()
  }
}

/**
 * A Media3 player backed by the Rust engine. The playlist is the current song between two
 * placeholders, so "previous" and "next" are always available and come back as seeks to them.
 * Also holds audio focus while playing (calls pause it, and it resumes after) and pauses when
 * headphones are unplugged.
 */
@OptIn(UnstableApi::class)
class RustPlayer(private val context: Context) : SimpleBasePlayer(Looper.getMainLooper()) {
  private val audio = context.getSystemService(AudioManager::class.java)
  private var hasFocus = false
  private var resumeOnGain = false
  private val focusRequest = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
    .setAudioAttributes(
      AudioAttributes.Builder()
        .setUsage(AudioAttributes.USAGE_MEDIA)
        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
        .build()
    )
    .setOnAudioFocusChangeListener(::onFocusChange)
    .build()

  private val noisy = object : BroadcastReceiver() {
    override fun onReceive(c: Context, intent: Intent) {
      if (intent.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY) NativeBridge.command("pause")
    }
  }
  private var noisyRegistered = false

  init {
    NativeBridge.listener = ::onEngineChanged
    NativeBridge.register()
  }

  private fun onEngineChanged() {
    val state = NativeBridge.state
    // Playback can start from the app's own buttons too, so focus follows the engine's state.
    if (state == NativeBridge.PLAYING && !hasFocus) {
      hasFocus = audio.requestAudioFocus(focusRequest) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED
      if (!hasFocus) NativeBridge.command("pause")
    } else if (state == NativeBridge.STOPPED && hasFocus) {
      audio.abandonAudioFocusRequest(focusRequest)
      hasFocus = false
    }
    val wantNoisy = state == NativeBridge.PLAYING
    if (wantNoisy != noisyRegistered) {
      if (wantNoisy) {
        context.registerReceiver(noisy, IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY), Context.RECEIVER_NOT_EXPORTED)
      } else {
        context.unregisterReceiver(noisy)
      }
      noisyRegistered = wantNoisy
    }
    invalidateState()
  }

  private fun onFocusChange(change: Int) {
    when (change) {
      AudioManager.AUDIOFOCUS_LOSS -> {
        resumeOnGain = false
        hasFocus = false
        audio.abandonAudioFocusRequest(focusRequest)
        NativeBridge.command("pause")
      }
      AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> {
        // A call or an alarm: pause now, play again when it's over.
        resumeOnGain = NativeBridge.state == NativeBridge.PLAYING
        NativeBridge.command("pause")
      }
      AudioManager.AUDIOFOCUS_GAIN -> {
        if (resumeOnGain) NativeBridge.command("play")
        resumeOnGain = false
      }
      // AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK: Android 12+ lowers the volume for us.
    }
  }

  override fun getState(): State {
    val commands = Player.Commands.Builder()
      .addAll(
        COMMAND_PLAY_PAUSE, COMMAND_STOP, COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM,
        COMMAND_SEEK_TO_NEXT, COMMAND_SEEK_TO_NEXT_MEDIA_ITEM,
        COMMAND_SEEK_TO_PREVIOUS, COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM,
        COMMAND_GET_CURRENT_MEDIA_ITEM, COMMAND_GET_METADATA, COMMAND_GET_TIMELINE,
      )
      .build()
    val track = NativeBridge.track
      ?: return State.Builder().setAvailableCommands(commands).setPlaybackState(STATE_IDLE).build()

    val meta = MediaMetadata.Builder()
      .setTitle(track.title)
      .setArtist(track.artist)
      .setAlbumTitle(track.album)
      .apply { track.cover?.let { setArtworkUri(Uri.fromFile(File(it))) } }
      .build()
    val current = MediaItemData.Builder("current")
      .setMediaItem(MediaItem.Builder().setMediaId("current").setMediaMetadata(meta).build())
      .setMediaMetadata(meta)
      .setDurationUs(track.durationMs * 1000)
      .build()

    val state = NativeBridge.state
    val playing = state == NativeBridge.PLAYING
    val position = NativeBridge.positionMs +
      if (playing) SystemClock.elapsedRealtime() - NativeBridge.positionAt else 0L
    return State.Builder()
      .setAvailableCommands(commands)
      .setPlaylist(listOf(placeholder("previous"), current, placeholder("next")))
      .setCurrentMediaItemIndex(1)
      .setPlayWhenReady(playing, PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST)
      .setPlaybackState(if (state == NativeBridge.STOPPED) STATE_IDLE else STATE_READY)
      .setContentPositionMs(PositionSupplier.getExtrapolating(position, if (playing) 1f else 0f))
      .build()
  }

  private fun placeholder(id: String) =
    MediaItemData.Builder(id).setMediaItem(MediaItem.Builder().setMediaId(id).build()).build()

  override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> {
    NativeBridge.command(if (playWhenReady) "play" else "pause")
    return Futures.immediateVoidFuture()
  }

  override fun handleSeek(mediaItemIndex: Int, positionMs: Long, seekCommand: Int): ListenableFuture<*> {
    when {
      seekCommand == COMMAND_SEEK_TO_NEXT || seekCommand == COMMAND_SEEK_TO_NEXT_MEDIA_ITEM || mediaItemIndex == 2 ->
        NativeBridge.command("next")
      // The engine restarts the song instead when it's past its first few seconds.
      seekCommand == COMMAND_SEEK_TO_PREVIOUS || seekCommand == COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM || mediaItemIndex == 0 ->
        NativeBridge.command("previous")
      else -> NativeBridge.command("seek", positionMs / 1000.0)
    }
    return Futures.immediateVoidFuture()
  }

  override fun handleStop(): ListenableFuture<*> {
    NativeBridge.command("stop")
    return Futures.immediateVoidFuture()
  }

  override fun handleRelease(): ListenableFuture<*> {
    NativeBridge.listener = null
    if (noisyRegistered) context.unregisterReceiver(noisy)
    noisyRegistered = false
    if (hasFocus) audio.abandonAudioFocusRequest(focusRequest)
    hasFocus = false
    return Futures.immediateVoidFuture()
  }
}
