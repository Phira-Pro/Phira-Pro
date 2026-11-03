package org.flos.phirapro;

import android.app.Activity;
import android.app.ProgressDialog;
import android.content.ClipData;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Handler;
import android.os.Looper;
import android.provider.DocumentsContract;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.widget.Toast;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.IOException;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.UUID;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.zip.ZipEntry;
import java.util.zip.ZipOutputStream;

/** Editor-only file and directory selection; never requests persistent access. */
final class RpenFilePicker {
    private static final int FILES=7301, FOLDER=7302;
    private static final String FOLDER_TYPE="application/vnd.phira.rpen-folder+zip";
    private static final String REFRESH_PREFIX="application/vnd.phira.rpen-folder-";
    private final Activity activity;
    private final Handler handler=new Handler(Looper.getMainLooper());
    private final ExecutorService worker=Executors.newSingleThreadExecutor();
    private final LinkedHashMap<String,Uri> selectedTrees=new LinkedHashMap<>();
    private ValueCallback<Uri[]> callback;
    private ProgressDialog progress;
    private volatile int generation;
    private boolean destroyed;
    RpenFilePicker(Activity activity){this.activity=activity;clearCache();}
    boolean show(ValueCallback<Uri[]> next,WebChromeClient.FileChooserParams params){
        cancel();clearCache();callback=next;
        boolean folder=false;
        for(String type:params.getAcceptTypes()){
            if(FOLDER_TYPE.equals(type))folder=true;
            else if(type.startsWith(REFRESH_PREFIX) && type.endsWith("+zip")){
                String token=type.substring(REFRESH_PREFIX.length(),type.length()-4);
                Uri tree=selectedTrees.get(token);
                if(tree!=null)readDirectory(tree);else finish(null);
                return true;
            }
        }
        Intent intent=new Intent(folder?Intent.ACTION_OPEN_DOCUMENT_TREE:Intent.ACTION_OPEN_DOCUMENT);
        if(!folder){intent.addCategory(Intent.CATEGORY_OPENABLE);intent.setType("*/*");intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE,params.getMode()==WebChromeClient.FileChooserParams.MODE_OPEN_MULTIPLE);}
        try{activity.startActivityForResult(intent,folder?FOLDER:FILES);}
        catch(android.content.ActivityNotFoundException error){finish(null);}
        return true;
    }
    boolean result(int request,int code,Intent data){
        if(request!=FILES && request!=FOLDER)return false;
        if(callback==null)return true;
        if(code!=Activity.RESULT_OK || data==null){finish(null);return true;}
        if(request==FOLDER){
            Uri tree=data.getData();if(tree==null){finish(null);return true;}
            readDirectory(tree);return true;
        }
        ClipData clip=data.getClipData();Uri[] selected;
        if(clip!=null){selected=new Uri[clip.getItemCount()];for(int i=0;i<selected.length;i++)selected[i]=clip.getItemAt(i).getUri();}
        else selected=WebChromeClient.FileChooserParams.parseResult(code,data);
        finish(selected);return true;
    }
    private void readDirectory(Uri tree){
            final int job=generation;
            progress=new ProgressDialog(activity);progress.setMessage("正在读取所选文件夹");progress.setIndeterminate(true);progress.setCancelable(true);progress.setOnCancelListener(dialog->cancel());progress.show();
            worker.execute(()->{
                File archive=null;
                try{
                    archive=archive(tree,job);File ready=archive;
                    handler.post(()->{
                        if(job!=generation || destroyed){ready.delete();return;}
                        selectedTrees.put(ready.getName().replace(".zip",""),tree);
                        while(selectedTrees.size()>8)selectedTrees.remove(selectedTrees.keySet().iterator().next());
                        dismiss();finish(new Uri[]{new Uri.Builder().scheme("content").authority(activity.getPackageName()+".rpenfolders").appendPath(ready.getName()).build()});
                    });
                }catch(Exception error){
                    if(archive!=null)archive.delete();
                    handler.post(()->{if(job!=generation || destroyed)return;dismiss();finish(null);Toast.makeText(activity,"无法读取文件夹，请选择可访问且不超过 512 MiB、20000 个文件的目录",Toast.LENGTH_LONG).show();});
                }
            });
    }
    private void finish(Uri[] result){ValueCallback<Uri[]> pending=callback;callback=null;if(pending!=null)pending.onReceiveValue(result);}
    private void dismiss(){if(progress!=null){progress.dismiss();progress=null;}}
    private void cancel(){generation++;dismiss();finish(null);}
    void destroy(){destroyed=true;cancel();worker.shutdownNow();selectedTrees.clear();clearCache();}
    private void clearCache(){File[] files=RpenFolderProvider.directory(activity).listFiles();if(files!=null)for(File file:files)if(file.getName().matches("[a-f0-9-]{36}\\.zip"))file.delete();}
    private void check(int job) throws IOException{if(generation!=job || Thread.currentThread().isInterrupted())throw new IOException("Cancelled");}
    private String name(String name) throws IOException{
        if(name==null || name.isEmpty() || name.equals(".") || name.equals("..") || name.indexOf('/')>=0 || name.indexOf('\\')>=0 || name.indexOf(':')>=0 || name.indexOf('\0')>=0)throw new IOException("Invalid document name");return name;
    }
    private File archive(Uri tree,int job) throws IOException {
        File directory=RpenFolderProvider.directory(activity);if(!directory.isDirectory() && !directory.mkdirs())throw new IOException("Cache unavailable");
        File file=new File(directory,UUID.randomUUID()+".zip");
        Uri root=DocumentsContract.buildDocumentUriUsingTree(tree,DocumentsContract.getTreeDocumentId(tree));
        String rootName;
        try(Cursor cursor=activity.getContentResolver().query(root,new String[]{DocumentsContract.Document.COLUMN_DISPLAY_NAME},null,null,null)){
            if(cursor==null || !cursor.moveToFirst())throw new IOException("Directory unavailable");rootName=name(cursor.getString(0));
        }
        boolean complete=false;
        try {
            try(ZipOutputStream zip=new ZipOutputStream(new FileOutputStream(file))){
                zip.setLevel(0);long[] totals={0,0};
                append(tree,DocumentsContract.getTreeDocumentId(tree),rootName,zip,new HashSet<>(),new HashSet<>(),totals,job,0);
                if(totals[0]==0)throw new IOException("Empty directory");check(job);
            }
            complete=true;
        }finally{if(!complete)file.delete();}
        return file;
    }
    private void append(Uri tree,String id,String path,ZipOutputStream zip,HashSet<String> visited,HashSet<String> paths,long[] totals,int job,int depth) throws IOException {
        check(job);if(depth>64 || !visited.add(id))throw new IOException("Invalid directory structure");
        Uri children=DocumentsContract.buildChildDocumentsUriUsingTree(tree,id);
        String[] columns={DocumentsContract.Document.COLUMN_DOCUMENT_ID,DocumentsContract.Document.COLUMN_DISPLAY_NAME,DocumentsContract.Document.COLUMN_MIME_TYPE};
        try(Cursor cursor=activity.getContentResolver().query(children,columns,null,null,null)){
            if(cursor==null)throw new IOException("Directory unavailable");
            while(cursor.moveToNext()){
                check(job);String child=cursor.getString(0),entry=path+"/"+name(cursor.getString(1));
                if(!paths.add(entry) || paths.size()>22000)throw new IOException("Too many documents");
                if(DocumentsContract.Document.MIME_TYPE_DIR.equals(cursor.getString(2))){append(tree,child,entry,zip,visited,paths,totals,job,depth+1);continue;}
                if(++totals[0]>20000)throw new IOException("Too many files");
                zip.putNextEntry(new ZipEntry(entry));
                try(InputStream input=activity.getContentResolver().openInputStream(DocumentsContract.buildDocumentUriUsingTree(tree,child))){
                    if(input==null)throw new IOException("File unavailable");byte[] buffer=new byte[65536];int count;
                    while((count=input.read(buffer))!=-1){check(job);totals[1]+=count;if(totals[1]>512L*1024*1024)throw new IOException("Directory too large");zip.write(buffer,0,count);}
                }zip.closeEntry();
            }
        }
    }
}
