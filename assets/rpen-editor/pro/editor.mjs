// GPL-3.0-only. Integration uses RPEN's public editor interface.
import './recovery.mjs';
import '../src/ui/app.mjs';
import { createChartExport, assetPath } from '../src/platform/files.mjs';
import { resourceReferences } from '../src/platform/files.mjs';
import { flushLocalPreferences } from '../src/platform/local-preferences.mjs';
import { TouchController } from './touch.mjs';
import { installPanels } from './panels.mjs';
import { installLibrary } from './library.mjs';
import { installFolders } from './folders.mjs';
import { installLinkedMigration } from './linked.mjs';

const host=globalThis.phiraEditor;
let hostPoll;
try {
  await host.boot.wait(new Promise(resolve=>{const check=()=>{if(globalThis.rpenMods)resolve();};hostPoll=setInterval(check,25);check();}),'正在初始化制谱器');
} finally { clearInterval(hostPoll); }
const {editor,manager,runtime}=globalThis.rpenMods;
await host.boot.wait(Promise.all([manager.ready,editor.startupReady]),'正在准备谱面库、皮肤和扩展');
await host.boot.wait(document.fonts.ready,'正在准备界面字体');
document.body.classList.add('phira-editor');
// Keep the mobile settings overview visible until the search field is tapped.
if(matchMedia('(pointer:coarse)').matches){const title=document.querySelector('#settings-title');title.tabIndex=-1;title.autofocus=true;}
const status=document.querySelector('#status');
const buttons=[];
const action=(label,fn,options={})=>{const button=document.createElement('button');button.type='button';button.textContent=label;button.title=options.title??label;button.onclick=async()=>{try{await fn();}catch(error){report(error);}};buttons.push(button);return button;};
const report=error=>{status.textContent=error.message;editor.notify(error.message,'error');};
installFolders(report);
await installLinkedMigration(host);
let busy=false,suspended=false,closed=false;
const setBusy=value=>{busy=value;for(const button of buttons)button.disabled=value;};
const save=async()=>{if(editor.hasDocument){await editor.save();if(editor.dirty)throw Error('工程仍有未保存修改，请保存后重试');}await flushLocalPreferences();};
async function close(){if(busy||closed)return;setBusy(true);try{editor.pause();editor.noiseEditor.flush();await save();await globalThis.rpenPrepareClose?.();document.documentElement.classList.add('pro-closing');await new Promise(resolve=>setTimeout(resolve,matchMedia('(prefers-reduced-motion: reduce)').matches?0:200));await host.request('close','{}');closed=true;}catch(error){document.documentElement.classList.remove('pro-closing');setBusy(false);report(error);}}
document.addEventListener('phira-editor-close',close);
host.requestClose=close;
const playButton=action('Pro 试玩',()=>openPlay());playButton.id='pro-play';
const returnButton=action('返回 Pro',close);returnButton.id='pro-return';
document.querySelector('header nav').append(...installLibrary({editor,host,action,save,isBusy:()=>busy,setBusy,status,report}),playButton,returnButton);

let touch;
const scrollButton=action('⇅',()=>touch.setMode(touch.mode==='pan'?'edit':'pan'));
scrollButton.id='pro-touch-scroll';scrollButton.className='pro-aux-action';
scrollButton.title='启用触控滑动编辑区域';scrollButton.setAttribute('aria-label',scrollButton.title);
scrollButton.innerHTML='<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 20V4m-4 4 4-4 4 4m4-4v16m-4-4 4 4 4-4"/></svg>';
const refreshModes=()=>{scrollButton.setAttribute('aria-pressed',String(touch.mode==='pan'));document.querySelectorAll('[data-input-mode]').forEach(b=>b.setAttribute('aria-pressed',String(b.dataset.inputMode===touch.mode)));};
touch=new TouchController(editor,refreshModes);refreshModes();
const commands=document.createElement('dialog');commands.className='pro-dialog';commands.dataset.modInteractive='true';
const commandTitle=document.createElement('h2');commandTitle.textContent='触控操作';commands.append(commandTitle);
const modes=document.createElement('div');modes.className='pro-command-grid';
for(const [mode,label] of [['edit','编辑'],['box','框选'],['multi','多选']]){const button=action(label,()=>{touch.setMode(mode);commands.close();});button.dataset.inputMode=mode;modes.append(button);}commands.append(modes);
const grid=document.createElement('div');grid.className='pro-command-grid';
const runCommand=actionName=>{const event=new CustomEvent('rpen-command',{detail:{action:actionName},cancelable:true});Object.assign(event,{key:'',code:'',ctrlKey:false,altKey:false,shiftKey:false,metaKey:false,repeat:false,isComposing:false});window.dispatchEvent(event);};
for(const [name,label] of [['AddTap','放置音符或取点'],['AddEvent','放置事件'],['SelectAll','全选'],['Copy','复制'],['Shear','剪切'],['Paste','粘贴'],['KeepTimePaste','原时间粘贴'],['PasteMirror','镜像粘贴'],['Delete','删除选中'],['NumberMirror','数值取反'],['NumberFill','数值填充'],['PageLeft','向左微调'],['PageRight','向右微调'],['PageUp','向上微调'],['PageDown','向下微调'],['LastBeat','上一细分拍'],['NextBeat','下一细分拍'],['Undo','撤销'],['Redo','重做'],['Esc','取消当前操作']])grid.append(action(label,()=>{commands.close();runCommand(name);}));
commands.append(grid,action('关闭',()=>commands.close()));document.body.append(commands);
const commandButton=action('触控操作',()=>{refreshModes();commands.showModal();});commandButton.id='pro-touch-commands';
// Skin organization may move game-ui. Follow that button without moving it.
const placeTouchControls=()=>{const gameUi=document.querySelector('#game-ui');if(scrollButton.nextElementSibling!==gameUi||commandButton.nextElementSibling!==scrollButton)gameUi.before(commandButton,scrollButton);};
new MutationObserver(placeTouchControls).observe(document.querySelector('.workspace'),{childList:true,subtree:true});placeTouchControls();
installPanels(editor);

const play=document.createElement('dialog');play.className='pro-dialog';play.dataset.modInteractive='true';
play.innerHTML='<form method="dialog"><h2>在 Pro 中试玩</h2><p>使用当前工程的独立快照。试玩不记录正常成绩。</p><label>开始位置 <select name="mode"><option value="current">当前播放头</option><option value="all">从头开始</option><option value="range">指定时间区间</option></select></label><div class="pro-range"><label>开始秒数 <input name="start" type="number" min="0" step="0.01"></label><label>结束秒数 <input name="end" type="number" min="0" step="0.01"></label></div><p class="pro-warning" role="status"></p><div class="pro-actions"><button value="cancel">取消</button><button type="button" class="primary" data-start>开始试玩</button></div></form>';
document.body.append(play);
const playMode=play.querySelector('[name=mode]');
const syncPlayRange=()=>{play.querySelector('.pro-range').hidden=playMode.value!=='range';};
playMode.addEventListener('change',syncPlayRange);syncPlayRange();
function openPlay(){if(!editor.hasDocument)throw Error('请先新建或打开一个工程');play.querySelector('[name=start]').value=Math.max(0,editor.chartSeconds()).toFixed(2);play.querySelector('[name=end]').value=editor.audio.duration?.toFixed(2)??'60';play.querySelector('.pro-warning').textContent='RPEN 的编辑工具扩展保留原有运行方式。自定义运行时效果需要 Pro 支持对应提供者。';play.showModal();}
play.querySelector('[data-start]').onclick=async()=>{
  if(busy)return;setBusy(true);
  try{
    editor.pause();editor.noiseEditor.flush();touch.cancel();
    const refs=resourceReferences(editor.chart,editor.assets,editor.chartName);
    if(!refs.song||!assetPath(editor.assets,refs.song,editor.chartName))throw Error('试玩需要先导入并指定谱面音乐');
    const unknown=Object.values(editor.chart.globalEvents?.types??{}).filter(type=>type.source?.kind==='mod' && type.source?.id!=='spatial-camera');
    if(unknown.length)throw Error('当前工程含 Pro 尚未实现的扩展运行时效果，请在编辑器中预览或导出备份');
    await save();
    const mode=play.querySelector('[name=mode]').value;
    const start=mode==='all'?0:mode==='current'?Math.max(0,editor.chartSeconds()):Number(play.querySelector('[name=start]').value);
    const end=mode==='range'?Number(play.querySelector('[name=end]').value):null;
    if(!Number.isFinite(start)||start<0||(end!==null&&(!Number.isFinite(end)||end<=start)))throw Error('请填写有效的试玩时间区间');
    const chart={...editor.chart,META:{...editor.chart.META,useRpen:true}};
    const {blob}=createChartExport(chart,editor.assets,editor.chartName,{format:'pez',compatibility:'next',includeUnusedFiles:true});
    status.textContent='正在准备试玩快照';
    const snapshot=await host.request('snapshot',blob,'application/zip');
    await host.request('play',JSON.stringify({snapshot:snapshot.id,start,end}));
    play.close();status.textContent='原生试玩中';suspended=true;editor.pause();
  }catch(error){report(error);}finally{setBusy(false);}
};

runtime.events.addEventListener('editor:save-succeeded',()=>{status.textContent='工程已保存';});
runtime.events.addEventListener('editor:save-failed',()=>{status.textContent='保存失败，请重试';});
document.addEventListener('visibilitychange',()=>{if(document.hidden){editor.pause();editor.noiseEditor.flush();}});
const viewport=()=>{const height=window.visualViewport?.height??innerHeight;document.documentElement.style.setProperty('--pro-height',`${Math.round(height)}px`);document.body.classList.toggle('pro-keyboard-editing',matchMedia('(pointer:coarse)').matches&&height<innerHeight*.7);editor.invalidate();};
window.visualViewport?.addEventListener('resize',viewport);window.addEventListener('resize',viewport);viewport();

// One request at a time; background editors stop polling until visible again.
async function control(){
  if(closed)return;
  try{
    const response=await fetch(new URL('pro/control',host.root));const state=await response.json();
    if(state.message)report(Error(state.message));
    if(state.state==='request-close')await close();
    if(state.state==='playing'&&!suspended){suspended=true;editor.pause();}
    if(state.state==='visible'&&suspended){suspended=false;editor.pause();status.textContent='已返回编辑位置';viewport();window.dispatchEvent(new Event('resize'));editor.invalidate();}
  }catch(error){status.textContent='本地服务连接中断，请保留当前窗口并重试';}
  setTimeout(control,suspended?500:350);
}
control();
await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
host.boot.ready();
