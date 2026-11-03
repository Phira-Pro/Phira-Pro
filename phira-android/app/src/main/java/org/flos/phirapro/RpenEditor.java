package org.flos.phirapro;

import android.app.Activity;
import android.content.Intent;
import android.graphics.Color;
import android.net.Uri;
import android.view.View;
import android.view.ViewGroup;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceRequest;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.webkit.JavascriptInterface;
import android.webkit.WebResourceError;
import android.os.Handler;
import android.os.Looper;
import android.view.Gravity;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;
import android.widget.FrameLayout;
import java.util.concurrent.atomic.AtomicBoolean;

/** Editor-only overlay. Files are selected by the system picker. */
final class RpenEditor {
    private final RpenFilePicker picker;
    private final Activity activity;
    private final FrameLayout root;
    private final WebView web;
    private final String origin;
    private final String address;
    private final LinearLayout loading;
    private final TextView loadingText;
    private final Button retry;
    private final Button safe;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private final AtomicBoolean bootExit = new AtomicBoolean();
    private boolean booting = true;
    private boolean destroyed;
    private final Runnable timeout = () -> failBoot("制谱器加载超时，可以重试或停用扩展重试");

    RpenEditor(Activity activity, String address) {
        this.activity=activity;
        picker=new RpenFilePicker(activity);
        this.address=address;
        Uri uri=Uri.parse(address);
        if (!"http".equals(uri.getScheme()) || !"127.0.0.1".equals(uri.getHost())) throw new IllegalArgumentException("Invalid editor address");
        origin="http://127.0.0.1:"+uri.getPort()+"/";
        root=new FrameLayout(activity);root.setBackgroundColor(Color.rgb(23,29,38));
        web=new WebView(activity);
        WebSettings settings=web.getSettings();settings.setJavaScriptEnabled(true);settings.setDomStorageEnabled(true);
        settings.setAllowFileAccess(false);settings.setAllowContentAccess(true);settings.setMediaPlaybackRequiresUserGesture(false);
        settings.setBuiltInZoomControls(false);settings.setDisplayZoomControls(false);settings.setSupportMultipleWindows(false);
        web.setBackgroundColor(Color.rgb(23,29,38));
        web.setWebViewClient(new WebViewClient(){
            @Override public void onPageStarted(WebView view,String url,android.graphics.Bitmap icon){startBoot();}
            @Override public void onReceivedError(WebView view,WebResourceRequest request,WebResourceError error){if(request.isForMainFrame())failBoot("无法载入制谱器，请重试或返回 Pro");}
            @Override public void onReceivedHttpError(WebView view,WebResourceRequest request,android.webkit.WebResourceResponse response){if(request.isForMainFrame())failBoot("无法载入制谱器，请重试或返回 Pro");}
            @Override public boolean shouldOverrideUrlLoading(WebView view,String url){return !url.startsWith(origin);}
            @Override public boolean shouldOverrideUrlLoading(WebView view,WebResourceRequest request){return !request.getUrl().toString().startsWith(origin);}
        });
        web.addJavascriptInterface(new Object(){
            @JavascriptInterface public void ready(){activity.runOnUiThread(()->{
                if(destroyed)return;booting=false;handler.removeCallbacks(timeout);loading.setVisibility(View.GONE);
            });}
            @JavascriptInterface public void closeBoot(){activity.runOnUiThread(()->requestBootExit());}
            @JavascriptInterface public boolean directoryPicker(){return true;}
        },"PhiraEditorShell");
        web.setWebChromeClient(new WebChromeClient(){
            @Override public boolean onShowFileChooser(WebView view,ValueCallback<Uri[]> callback,FileChooserParams params){
                return picker.show(callback,params);
            }
        });
        root.addView(web,new FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT,ViewGroup.LayoutParams.MATCH_PARENT));
        loading=new LinearLayout(activity);loading.setOrientation(LinearLayout.VERTICAL);loading.setGravity(Gravity.CENTER);loading.setPadding(24,16,24,16);loading.setBackgroundColor(Color.rgb(23,29,38));
        loadingText=new TextView(activity);loadingText.setTextColor(Color.WHITE);loadingText.setTextSize(16);loadingText.setGravity(Gravity.CENTER);loading.addView(loadingText);
        LinearLayout actions=new LinearLayout(activity);actions.setGravity(Gravity.CENTER);loading.addView(actions);
        Button back=new Button(activity);back.setAllCaps(false);back.setText("返回 Pro");back.setOnClickListener(v->requestBootExit());actions.addView(back);
        retry=new Button(activity);retry.setText("重新加载");retry.setOnClickListener(v->web.reload());actions.addView(retry);
        safe=new Button(activity);safe.setText("停用扩展重试");safe.setOnClickListener(v->web.loadUrl(Uri.parse(address).buildUpon().appendQueryParameter("mods","off").build().toString()));actions.addView(safe);
        root.addView(loading,new FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT,ViewGroup.LayoutParams.MATCH_PARENT));
        activity.addContentView(root,new ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT,ViewGroup.LayoutParams.MATCH_PARENT));
        startBoot();
        web.loadUrl(address);
    }
    private void startBoot(){if(destroyed)return;booting=true;loading.setVisibility(View.VISIBLE);loadingText.setText("正在进入制谱器");retry.setVisibility(View.GONE);safe.setVisibility(View.GONE);handler.removeCallbacks(timeout);handler.postDelayed(timeout,20000);}
    private void failBoot(String message){if(destroyed||!booting)return;loadingText.setText(message);retry.setVisibility(View.VISIBLE);safe.setVisibility(View.VISIBLE);}
    private void requestBootExit(){if(!destroyed&&booting)bootExit.set(true);}
    boolean takeBootExit(){return bootExit.getAndSet(false);}
    void visible(boolean visible){root.setVisibility(visible?View.VISIBLE:View.GONE);if(visible){web.onResume();web.requestFocus();}else{web.evaluateJavascript("globalThis.rpenMods?.editor.pause()",null);web.onPause();}}
    boolean back(){if(root.getVisibility()!=View.VISIBLE)return false;if(booting)requestBootExit();else web.evaluateJavascript("globalThis.phiraEditor?.requestClose()",null);return true;}
    boolean result(int request,int code,Intent data){
        return picker.result(request,code,data);
    }
    void destroy(){destroyed=true;handler.removeCallbacks(timeout);picker.destroy();web.stopLoading();web.removeJavascriptInterface("PhiraEditorShell");web.destroy();if(root.getParent()!=null)((ViewGroup)root.getParent()).removeView(root);}
}
