package app

import java.nio.file.{Files, Paths}
import scala.jdk.CollectionConverters.*

/** Minimal reader for the cardano-init `.env` connection seam.
  *
  * Values are gathered from the given files (first occurrence of a key wins) and then overlaid with
  * the real process environment, which always takes precedence. The shared `../.env` carries
  * `CARDANO_NETWORK` and the connection details (`INDEXER_URL`, ...); the gitignored `.env.local`
  * carries secrets (`BLOCKFROST_API_KEY`, `MNEMONIC`). This reader never names the tool that wrote
  * `../.env` — it only reads the standard keys.
  */
object DotEnv {

    private val defaultFiles = Seq("../.env", ".env.local", ".env")

    /** Load key/value pairs from the given env files, overlaid with `System.getenv`. */
    def load(files: Seq[String] = defaultFiles): Map[String, String] = {
        val fromFiles = scala.collection.mutable.LinkedHashMap.empty[String, String]
        for (file <- files) {
            val path = Paths.get(file)
            if (Files.exists(path)) {
                for (line <- Files.readAllLines(path).asScala) {
                    parseLine(line).foreach { case (key, value) =>
                        if (!fromFiles.contains(key)) fromFiles(key) = value
                    }
                }
            }
        }
        // The real environment always overrides file values.
        (fromFiles ++ System.getenv().asScala).toMap
    }

    /** Parse `KEY=value`, ignoring blank lines and `#` comments, stripping surrounding quotes. */
    private def parseLine(raw: String): Option[(String, String)] = {
        val line = raw.trim
        if (line.isEmpty || line.startsWith("#")) None
        else {
            val idx = line.indexOf('=')
            if (idx <= 0) None
            else {
                val key = line.substring(0, idx).trim
                var value = line.substring(idx + 1).trim
                val quoted = value.length >= 2 &&
                    ((value.startsWith("\"") && value.endsWith("\"")) ||
                        (value.startsWith("'") && value.endsWith("'")))
                if (quoted) value = value.substring(1, value.length - 1)
                if (key.matches("[A-Za-z_][A-Za-z0-9_]*")) Some(key -> value) else None
            }
        }
    }
}
