package app.ownnotes.desktop

import android.content.Context
import android.os.Bundle
import android.view.inputmethod.InputMethodManager
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  // Read by the page at startup, because the first insets can arrive before it loads.
  @Volatile private var insets = ""
  private var webView: WebView? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }

  override fun onWebViewCreate(webView: WebView) {
    this.webView = webView
    webView.addJavascriptInterface(Bridge(), "OwnNotesAndroid")
    ViewCompat.setOnApplyWindowInsetsListener(webView) { view, windowInsets ->
      val density = resources.displayMetrics.density
      val bars = windowInsets.getInsets(
        WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
      )
      val keyboard = windowInsets.getInsets(WindowInsetsCompat.Type.ime())
      val top = (bars.top / density).toInt()
      val bottom = (bars.bottom / density).toInt()
      val typing = (keyboard.bottom / density).toInt()
      insets = "$top,$bottom,$typing"
      (view as WebView).evaluateJavascript(
        "window.__applyAndroidInsets && window.__applyAndroidInsets($top,$bottom,$typing)",
        null
      )
      windowInsets
    }
  }

  private inner class Bridge {
    @JavascriptInterface
    fun insets(): String = this@MainActivity.insets

    /** A label-focused search field does not always open the IME on its own. */
    @JavascriptInterface
    fun showKeyboard() {
      runOnUiThread {
        val view = webView ?: return@runOnUiThread
        val imm = getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
        view.requestFocus()
        imm.showSoftInput(view, InputMethodManager.SHOW_IMPLICIT)
      }
    }

    /** Dark status-bar icons for a light app theme, and the other way round. */
    @JavascriptInterface
    fun setLightBars(light: Boolean) {
      runOnUiThread {
        val controller = WindowCompat.getInsetsController(window, window.decorView)
        controller.isAppearanceLightStatusBars = light
        controller.isAppearanceLightNavigationBars = light
      }
    }
  }
}
