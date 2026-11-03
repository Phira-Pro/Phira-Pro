import { CAMERA_ID, round, clampAxis, valueDraft } from './camera.mjs';
import { CameraStudio } from './studio.mjs';
import { beatValue } from '@rpen/core/beat.mjs';



import { cameraFollowsChart, withCameraFollow } from './settings.mjs';

const element=(tag,className,text)=>{const node=document.createElement(tag);if(className)node.className=className;if(text)node.textContent=text;return node;};
const button=(text,run)=>{const node=element('button','',text);node.type='button';node.onclick=run;return node;};
export class CameraControls {
  constructor(api,tracks){
    this.api=api;this.editor=api.editor;this.tracks=tracks;this.stage=document.querySelector('.stage');this.endpoints=new WeakMap();
    this.surface=element('div','camera3d-sampler');this.surface.dataset.modInteractive='true';this.surface.hidden=true;this.surface.setAttribute('aria-label','拖动调整摄像机');
    this.bar=element('div','camera3d-sample-bar');this.bar.dataset.modInteractive='true';this.bar.hidden=true;
    this.sampleValue=element('strong');this.sampleHint=element('span');this.bar.append(this.sampleValue,this.sampleHint,button('应用',()=>this.finishSample(true)),button('取消',()=>this.finishSample(false)));
    api.mount(this.surface,this.stage);api.mount(this.bar,this.stage);
    api.listen(this.surface,'pointerdown',event=>this.down(event));api.listen(this.surface,'pointermove',event=>this.move(event));api.listen(this.surface,'pointerup',()=>{this.drag=null;});api.listen(this.surface,'pointercancel',()=>{this.drag=null;});
    api.listen(this.surface,'wheel',event=>{if(!this.sample)return;event.preventDefault();const editor=this.editor;editor.playback.wheel(event,{...editor.preferences.settings,...editor.editorPreferences},performance.now()/1000);this.sample.seconds=editor.chartSeconds();this.api.editor.invalidate();},{passive:false});
    api.hook('input:before',input=>{if(this.visualActive&&input.event.type==='keydown'&&input.event.key==='Escape'){this.finishSample(false);this.closeAssistant();input.handled=true;}},100);
    api.hook('preview:before',view=>{
      if(this.sample){if(this.editor.session!==this.sample.session||this.editor.chart!==this.sample.base){this.finishSample(false);return;}view.chart=this.sample.chart;view.seconds=this.sample.seconds;}
      else if(this.assistant&&this.assistant.session===this.editor.session){view.seconds=this.assistant.seconds;}
    },25);
    api.onFrame(({elapsed})=>{
      if(this.sample&&!this.editor.hasDocument)this.finishSample(false);
      if(this.assistant&&this.editor.session!==this.assistant.session)this.closeAssistant();
      if(this.assistant?.playing){this.assistant.progress=(this.assistant.progress+Math.min(.1,elapsed)/3)%1;this.progress.value=this.assistant.progress;this.updateAssistant();}
    });
    api.dispose(()=>{this.finishSample(false);this.closeAssistant();});
  }
  get visualActive(){return Boolean(this.sample||this.assistant);}
  get poseOverride(){return this.assistant?.pose;}
  view(){const renderer=this.editor.preview.visible?this.editor.preview:this.editor.realtimePreview;return{renderer,scale:renderer.viewScale||1,rect:renderer.canvas.getBoundingClientRect()};}
  beginView(){const renderer=this.editor.realtimePreview;const previous={visible:renderer.visible,opacity:renderer.opacity};renderer.visible=true;renderer.opacity=1;return()=>{renderer.visible=previous.visible;renderer.opacity=previous.opacity;this.editor.invalidate();};}
  lock(reason){return this.api.lockEditing({reason,roots:['header nav > :not(#mods-open)','.transport','.editor-toolbar','.timeline-head','.editor-footer','.inspector','.canvases','.timeline-strips']});}
  inspector(axis){return context=>{
    let saved=this.endpoints.get(this.editor.session);if(!saved){saved=new Map();this.endpoints.set(this.editor.session,saved);}const key=context.entries[0].type+':'+context.entries[0].index;
    const form=context.container.closest('form');
    form.insertBefore(context.container,form.children[1]);
    for(const key of ['start','end'])form.querySelector(`[data-rpen-field="${key}"]`).step='any';
    const host=element('section','camera3d-inspector');host.dataset.cameraAxis=axis.id;
    const title=element('h4','',axis.name+' · 直观调整');host.append(title);
    const endpoints=element('div','camera3d-endpoints');let selected=saved.get(key)??'end';
    const slider=element('input');slider.type='range';slider.step=axis.step;slider.min=Math.min(axis.slider[0],context.event.start,context.event.end);slider.max=Math.max(axis.slider[1],context.event.start,context.event.end);slider.setAttribute('aria-label',axis.name+'拖动调节');
    const value=element('output');
    const syncFields=()=>{const current=this.currentEvent(context);for(const key of ['start','end']){const input=form.querySelector(`[data-rpen-field="${key}"]`);if(input&&document.activeElement!==input)input.value=current[key];}};
    const sync=()=>{const current=this.currentEvent(context);slider.value=current?.[selected]??axis.fallback;value.textContent=round(Number(slider.value))+axis.unit;for(const [key,node]of endpointButtons)node.setAttribute('aria-pressed',String(key===selected));syncFields();};
    const endpointButtons=new Map();for(const [endpoint,name]of [['start','起始值'],['end','结束值']]){const control=button(name,()=>{selected=endpoint;saved.set(key,endpoint);sync();const event=this.currentEvent(context);this.editor.seekBeat(beatValue(event[endpoint==='start'?'startTime':'endTime']));});control.dataset.cameraEndpoint=endpoint;endpointButtons.set(endpoint,control);endpoints.append(control);}host.append(endpoints,slider,value);
    const seek=()=>{const event=this.currentEvent(context);if(event)this.editor.seekBeat(beatValue(event[selected==='start'?'startTime':'endTime']));};
    slider.oninput=()=>{context.update(event=>({...event,[selected]:Number(slider.value),...(event.inst?{start:Number(slider.value),end:Number(slider.value)}:{})}),'调整摄像机'+axis.name);value.textContent=round(Number(slider.value))+axis.unit;syncFields();};
    const presets=element('div','camera3d-presets');for(const number of axis.presets)presets.append(button(round(number)+axis.unit,()=>{context.update(event=>({...event,[selected]:number,...(event.inst?{start:number,end:number}:{})}),'摄像机'+axis.name+'快捷值');sync();}));host.append(presets);
    const actions=element('div','camera3d-actions');actions.append(button('定位此端点',seek),button('在预览中拖拽',()=>this.beginSample(axis,context,selected)));host.append(actions);
    const hint=element('p','hint','X 向右，Y 向上，谱面位于 Z=0；偏航向右、俯仰向上，倾斜正值让画面顺时针转动。调整仅影响当前列。');host.append(hint);context.container.append(host);sync();
    slider.onpointerdown=()=>{slider.focus({preventScroll:true});seek();};
    return this.api.listen(host,'focusout',()=>queueMicrotask(()=>{if(host.isConnected&&!host.contains(document.activeElement))this.editor.globalEvents.inspector.render();}));
  };}
  followControl(){const label=element('label','camera3d-follow');const input=element('input');input.type='checkbox';input.dataset.cameraFollowChart='true';input.checked=cameraFollowsChart(this.editor.chart);input.onchange=()=>{this.editor.commit('修改摄像机跟随谱面',withCameraFollow(this.editor.chart,this.api.manifest,input.checked));this.editor.invalidate();};label.append(input,'本谱面：UI 和背景跟随谱面');return label;}
  overviewControls(){const section=element('section','camera3d-overview-tools');section.id='spatial-camera-overview';section.append(element('h3','','三维摄像机'));const assistant=button('姿态过渡助手',()=>this.openAssistant());assistant.id='spatial-camera-assistant';assistant.className='wide-button';assistant.title='直接编辑摄像机首尾姿态 · 默认 Ctrl+Alt+C';section.append(assistant,this.followControl(),element('p','hint','跟随选项仅影响当前谱面，随谱面保存，可撤销。'));return section;}
  currentEvent(context){const entry=context.entries[0];return this.editor.chart.globalEvents?.types?.[CAMERA_ID+'/'+context.column.cameraType]?.columns?.[context.column.id]?.events?.[entry.index]??context.event;}
  beginSample(axis,context,which){
    this.finishSample(false);this.closeAssistant();const editor=this.editor,event=this.currentEvent(context),index=context.entries[0].index,type=CAMERA_ID+'/'+axis.type;
    if(!event)return;editor.pause();editor.seekBeat(beatValue(event[which==='start'?'startTime':'endTime']));
    const restoreView=this.beginView();const unlock=this.lock('调整摄像机');
    this.sample={axis,index,type,field:which,base:editor.chart,chart:editor.chart,session:editor.session,value:event[which],seconds:editor.chartSeconds(),restoreView,unlock};
    this.stage.closest('.editor').classList.add('camera3d-adjusting');this.surface.hidden=this.bar.hidden=false;this.sampleHint.textContent=axis.id==='roll'?'围绕中心拖动调整角度':'拖动调整 · Esc 取消 · 滚轮调整时间';this.syncSample();editor.invalidate();
  }
  syncSample(){this.sampleValue.textContent=this.sample.axis.name+' '+round(this.sample.value)+this.sample.axis.unit;}
  down(event){if(!this.sample||event.button!==0)return;event.preventDefault();const view=this.view();this.drag={x:event.clientX,y:event.clientY,value:this.sample.value,scale:view.scale,angle:Math.atan2(event.clientY-view.rect.top-view.rect.height/2,event.clientX-view.rect.left-view.rect.width/2)};this.surface.setPointerCapture(event.pointerId);}
  move(event){
    const sample=this.sample,drag=this.drag;if(!sample||!drag)return;const dx=event.clientX-drag.x,dy=event.clientY-drag.y,axis=sample.axis;let delta;
    if(axis.id==='x')delta=dx/drag.scale;
    else if(axis.id==='y')delta=-dy/drag.scale;
    else if(axis.id==='roll'){const view=this.view(),angle=Math.atan2(event.clientY-view.rect.top-view.rect.height/2,event.clientX-view.rect.left-view.rect.width/2);delta=Math.atan2(Math.sin(angle-drag.angle),Math.cos(angle-drag.angle))*180/Math.PI;}
    else delta=(axis.id==='yaw'?dx:axis.id==='pitch'?-dy:dy)*axis.drag;
    sample.value=round(clampAxis(axis,drag.value+delta));sample.chart=valueDraft(sample.base,sample.type,axis.id,sample.index,sample.field,sample.value);this.syncSample();this.editor.invalidate();
  }
  finishSample(apply){
    const sample=this.sample;if(!sample)return;this.sample=null;this.drag=null;this.surface.hidden=this.bar.hidden=true;this.stage.closest('.editor').classList.remove('camera3d-adjusting');sample.unlock();sample.restoreView();
    if(apply&&this.editor.session===sample.session&&this.editor.chart===sample.base){const events=sample.chart.globalEvents.types[sample.type].columns[sample.axis.id].events;this.api.globalEvents.replace(sample.type,sample.axis.id,events,'应用摄像机拖拽调整');}
    this.editor.invalidate();
  }
  openAssistant(){
    if(!this.editor.hasDocument){this.api.notify('请先打开谱面','warning');return;}
    this.finishSample(false);this.closeAssistant();this.editor.pause();this.editor.eventSampler.finish(false);this.editor.noiseEditor.endSample(false);
    try{this.studio=new CameraStudio(this);}catch(error){this.closeAssistant();this.api.notify(error.message,'error');}
  }
  updateAssistant(){this.studio?.update(false);}
  closeAssistant(){const studio=this.studio;this.studio=null;if(studio)studio.dispose();else if(this.assistant){this.assistant.unlock?.();this.assistant.restoreView?.();this.dialog?.remove();this.stage.closest('.editor').classList.remove('camera3d-adjusting');}this.assistant=null;this.dialog=null;this.editor.invalidate();}
}
