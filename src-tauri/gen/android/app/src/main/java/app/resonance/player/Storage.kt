package app.resonance.player

import android.content.Context
import android.net.Uri
import android.os.Environment
import android.provider.DocumentsContract
import java.io.File

/**
 * Music folders on the phone (docs/android-design.md §4). Songs are read by path with the audio
 * permission; folder pictures, which that permission hides, are read through the grant the
 * folder picker gave.
 */
object Storage {
  private const val PROVIDER = "com.android.externalstorage.documents"
  private val ART_NAMES = listOf("cover", "folder", "front", "album", "albumart", "albumartlarge", "art")
  private val ART_EXTS = setOf("jpg", "jpeg", "png", "webp", "bmp", "gif")
  private const val ART_MAX_BYTES = 20 * 1024 * 1024

  /**
   * The real path of a folder picked in the system picker: "primary:Music/Albums" is
   * /storage/emulated/0/Music/Albums, "1A2B-3C4D:Music" is /storage/1A2B-3C4D/Music. Folders
   * from other providers (Downloads, cloud drives) have no path: null.
   */
  fun treePath(tree: Uri): String? {
    if (tree.authority != PROVIDER) return null
    val id = DocumentsContract.getTreeDocumentId(tree)
    val volume = id.substringBefore(':')
    val rel = id.substringAfter(':', "")
    val root = if (volume == "primary") Environment.getExternalStorageDirectory() else File("/storage", volume)
    val dir = if (rel.isEmpty()) root else File(root, rel)
    return dir.takeIf { it.isDirectory }?.absolutePath
  }

  /** The folder picture (cover.jpg, folder.png, ...) of `dir`, through a picked folder's grant. */
  fun folderImage(context: Context, dir: String): ByteArray? {
    val resolver = context.contentResolver
    for (grant in resolver.persistedUriPermissions) {
      val tree = grant.uri
      if (!grant.isReadPermission || tree.authority != PROVIDER) continue
      val root = treePath(tree) ?: continue
      if (dir != root && !dir.startsWith("$root/")) continue
      val treeId = DocumentsContract.getTreeDocumentId(tree)
      val rel = dir.removePrefix(root).trim('/')
      val dirId = when {
        rel.isEmpty() -> treeId
        treeId.endsWith(":") -> treeId + rel
        else -> "$treeId/$rel"
      }
      val children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, dirId)
      var best: Pair<Int, String>? = null
      var anyImage: String? = null
      resolver.query(
        children,
        arrayOf(DocumentsContract.Document.COLUMN_DOCUMENT_ID, DocumentsContract.Document.COLUMN_DISPLAY_NAME),
        null, null, null,
      )?.use { c ->
        while (c.moveToNext()) {
          val name = c.getString(1) ?: continue
          if (name.substringAfterLast('.', "").lowercase() !in ART_EXTS) continue
          val rank = ART_NAMES.indexOf(name.substringBeforeLast('.').lowercase())
          if (rank >= 0 && (best == null || rank < best!!.first)) best = rank to c.getString(0)
          if (anyImage == null) anyImage = c.getString(0)
        }
      }
      val doc = best?.second ?: anyImage ?: return null
      return resolver.openInputStream(DocumentsContract.buildDocumentUriUsingTree(tree, doc))?.use { input ->
        val bytes = input.readNBytes(ART_MAX_BYTES + 1)
        bytes.takeIf { it.size <= ART_MAX_BYTES }
      }
    }
    return null
  }
}
