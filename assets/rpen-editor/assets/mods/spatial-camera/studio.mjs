import { AXES, DEFAULT_POSE, POSES, poseValues, round } from './camera.mjs';
import { CameraScene, lookAtOrigin } from './scene.mjs';
import { transitionChanges } from './events.mjs';
import { Preview } from '@rpen/ui/preview.mjs';
import { beatValue, parseBeat, formatBeat, fromNumber } from '@rpen/core/beat.mjs';
import { beatInputControl } from '@rpen/ui/beat-input.mjs';
import { easing } from '@rpen/core/easing.mjs';
import { EASING_NAMES } from '@rpen/ui/easing-picker.mjs';

const node=(tag,cls,text)=>{const result=document.createElement(tag);if(cls)result.className=cls;if(text)result.textContent=text;return result;};
const button=(text,run)=>{const result=node('button','',text);result.type='button';result.onclick=run;return result;};
const field=(host,name,input)=>{const label=node('label','field',name);label.append(input);host.append(label);return input;};

/** Draft-only two-camera authoring workspace; generation remains one chart transaction. */
export class CameraStudio {
  constructor(controls){
    this.controls=controls;this.api=controls.api;this.editor=controls.editor;this.closed=false;
    const editor=this.editor,beat=editor.tempo.beat(editor.chartSeconds()),first=controls.tracks.sample(editor.chart,editor.tempo,editor.chartSeconds());
    const state=this.state=controls.assistant={session:editor.session,start:null,end:null,ease:null,poses:{start:{...first},end:{...POSES.orbitLeft.pose}},active:'end',progress:1,playing:false,pose:{...POSES.orbitLeft.pose},seconds:editor.chartSeconds(),restoreView:controls.beginView(),unlock:controls.lock('编辑三维摄像机姿态')};
    controls.stage.closest('.editor').classList.add('camera3d-adjusting');
    const dialog=this.dialog=controls.dialog=node('dialog','camera3d-dialog camera3d-studio');dialog.dataset.modInteractive='true';document.body.append(dialog);
    this.unmount=this.api.dispose(()=>dialog.remove());this.listeners=[];this.inputs=new Map();this.endpointButtons=new Map();this.views=new Map();
    const heading=node('div','camera3d-heading');heading.append(node('div','', '三维摄像机 · 姿态过渡'),button('关闭',()=>controls.closeAssistant()));dialog.append(heading);
    const layout=node('div','camera3d-studio-layout'),world=node('section','camera3d-world'),panel=node('section','camera3d-studio-panel');layout.append(world,panel);dialog.append(layout);
    const tools=node('div','camera3d-world-tools');this.modeButtons=new Map();for(const [id,name]of [['move','移动'],['rotate','旋转']]){const control=button(name,()=>{this.scene.mode=id;for(const [key,b]of this.modeButtons)b.setAttribute('aria-pressed',String(key===id));this.scene.draw();});control.dataset.cameraSceneTool=id;control.setAttribute('aria-pressed',String(id==='move'));this.modeButtons.set(id,control);tools.append(control);}tools.append(button('框住全部',()=>this.scene.frameAll()),button('重置观察视角',()=>{this.scene.view={yaw:-35,pitch:-24,distance:1600,target:[0,0,350]};this.scene.draw();}));world.append(tools);
    this.canvas=node('canvas','camera3d-world-canvas');this.canvas.tabIndex=0;this.canvas.dataset.cameraScene='true';this.canvas.setAttribute('aria-label','三维摄像机姿态编辑场景');world.append(this.canvas);
    world.append(node('p','hint','左键拖动轴、圆环或摄像机本体调整；拖空白 / 右键旋转观察，Shift+右键 / 中键平移，滚轮缩放观察；Shift 拖动可吸附。'));
    const shots=node('div','camera3d-shots');for(const key of ['start','end']){
      const card=node('section','camera3d-shot');card.dataset.cameraPose=key;const select=button(key==='start'?'编辑起始姿态':'编辑结束姿态',()=>this.select(key));select.dataset.cameraStudioEndpoint=key;this.endpointButtons.set(key,select);card.append(select);
      const image=node('canvas','camera3d-pose-preview');image.setAttribute('aria-label',key==='start'?'起始摄像机画面':'结束摄像机画面');card.append(image);shots.append(card);this.views.set(key,{card,canvas:image});
    }world.append(shots);
    const start=state.start=node('input'),end=state.end=node('input');start.value=formatBeat(fromNumber(beat));end.value=formatBeat(fromNumber(beat+4));start.setAttribute('aria-label','摄像机过渡开始拍');end.setAttribute('aria-label','摄像机过渡结束拍');
    field(panel,'开始拍',beatInputControl(start,()=>editor.timeline.division,error=>this.fail(error)));field(panel,'结束拍',beatInputControl(end,()=>editor.timeline.division,error=>this.fail(error)));
    for(const input of [start,end]){input.oninput=()=>this.update();input.onchange=()=>this.update();}
    const endpoints=node('div','camera3d-studio-switch');for(const key of ['start','end']){const control=button(key==='start'?'调整起始姿态':'调整结束姿态',()=>this.select(key));control.dataset.cameraStudioSwitch=key;endpoints.append(control);}panel.append(endpoints);
    this.activeTitle=node('h3');panel.append(this.activeTitle);const preset=this.preset=node('select');preset.setAttribute('aria-label','当前端点姿态预设');preset.append(new Option('自定义姿态','custom'),new Option('读取该拍的现有姿态','current'));for(const [id,value]of Object.entries(POSES))preset.append(new Option(value.name,id));preset.value='orbitLeft';preset.onchange=()=>{if(preset.value==='custom')return;try{const key=state.active,values=this.values(),pose=preset.value==='current'?controls.tracks.sample(editor.chart,editor.tempo,editor.tempo.seconds(key==='start'?values.start:values.end)):POSES[preset.value].pose;this.editPose({...pose},false);}catch(error){this.fail(error);}};field(panel,'应用预设',preset);
    const poseFields=node('div','camera3d-pose-fields');panel.append(poseFields);
    for(const axis of AXES){const input=node('input');input.type='number';input.step='any';input.min=axis.min;input.max=axis.max;input.dataset.cameraStudioValue=axis.id;input.setAttribute('aria-label','姿态'+axis.name);input.oninput=()=>{const value=input.value.trim()?Number(input.value):NaN;if(!Number.isFinite(value)||value<axis.min||value>axis.max){input.setCustomValidity('请输入范围内的数值');this.generateButton.disabled=true;return;}input.setCustomValidity('');this.editPose({...state.poses[state.active],[axis.id]:value});};this.inputs.set(axis.id,input);field(poseFields,axis.name+axis.unit,input);}
    const actions=node('div','camera3d-actions');actions.append(button('朝向谱面中心',()=>this.editPose(lookAtOrigin(state.poses[state.active]))),button('复制另一端姿态',()=>this.editPose({...state.poses[state.active==='start'?'end':'start']})),button('交换首尾',()=>{[state.poses.start,state.poses.end]=[state.poses.end,state.poses.start];this.update();}));panel.append(actions);
    panel.append(node('p','hint','姿态修改在生成前仅为草稿。较暗的摄像机和画面表示另一端。'));
    const ease=state.ease=node('select');ease.setAttribute('aria-label','摄像机过渡缓动');EASING_NAMES.forEach((name,index)=>ease.append(new Option(`${index+1} · ${name}`,index+1)));ease.onchange=()=>this.update();field(panel,'缓动',ease);
    const preview=node('div','camera3d-preview-controls');this.progress=controls.progress=node('input');this.progress.type='range';this.progress.min=0;this.progress.max=1;this.progress.step=.001;this.progress.value=1;this.progress.setAttribute('aria-label','过渡预览进度');this.progress.oninput=()=>{state.playing=false;state.progress=Number(this.progress.value);this.update(false);};preview.append(button('动态预览过渡',()=>{state.playing=!state.playing;}),this.progress);panel.append(preview);
    this.transitionCanvas=node('canvas','camera3d-transition-preview');this.transitionCanvas.setAttribute('aria-label','摄像机过渡实时画面');panel.append(this.transitionCanvas);
    this.readout=node('output','camera3d-readout');this.message=node('p','hint');panel.append(this.readout,this.message);
    this.errorAlert=node('div','camera3d-studio-error');this.errorAlert.hidden=true;this.errorAlert.tabIndex=-1;this.errorAlert.setAttribute('role','alert');this.errorAlert.setAttribute('aria-live','assertive');dialog.append(this.errorAlert);
    const footer=node('div','camera3d-dialog-actions');this.generateButton=button('生成摄像机事件',()=>this.generate());this.generateButton.className='primary';this.generateButton.dataset.cameraGenerate='true';footer.append(node('span','hint','七轴分别缓动 · 一次生成，一步撤销'),button('取消姿态编辑',()=>controls.closeAssistant()),this.generateButton);dialog.append(footer);
    this.scene=new CameraScene(this.canvas,{change:pose=>this.editPose(pose),select:key=>this.select(key)});this.scene.aspect=editor.preview.aspectRatio??1.5;
    this.preview=new Preview(document.createElement('canvas'));this.releasePreview=controls.attachRenderer(this.preview);this.preview.invalidate=()=>this.requestViews();this.preview.noisePreview.invalidate=()=>this.requestViews();this.preview.noisePreview.getAssets=()=>editor.assets;
    this.plane=node('canvas');this.plane.getBoundingClientRect=()=>this.views.get('start').canvas.getBoundingClientRect();
    this.listeners.push(this.api.listen(dialog,'cancel',event=>{event.preventDefault();controls.closeAssistant();}));
    this.resize=new ResizeObserver(()=>this.requestViews());this.resize.observe(shots);
    dialog.showModal();this.update();
  }
  values(){const s=this.state,start=beatValue(parseBeat(s.start.value)),end=beatValue(parseBeat(s.end.value));if(end<=start)throw Error('结束拍必须晚于开始拍');return{start,end,first:s.poses.start,last:s.poses.end,easingType:Number(s.ease.value)};}
  select(key){this.state.active=key;this.state.progress=key==='start'?0:1;this.state.playing=false;this.progress.value=this.state.progress;this.preset.value='custom';this.update();}
  editPose(pose,custom=true){this.state.poses[this.state.active]=poseValues(pose);if(custom)this.preset.value='custom';this.state.progress=this.state.active==='start'?0:1;this.progress.value=this.state.progress;this.state.playing=false;this.update();}
  update(views=true){
    if(this.closed)return;if(views){this.errorAlert.hidden=true;this.errorAlert.textContent='';}try{
      const state=this.state,values=this.values(),amount=easing(state.progress,values.easingType);state.pose=Object.fromEntries(AXES.map(axis=>[axis.id,values.first[axis.id]+(values.last[axis.id]-values.first[axis.id])*amount]));state.seconds=this.editor.tempo.seconds(values.start)+(this.editor.tempo.seconds(values.end)-this.editor.tempo.seconds(values.start))*state.progress;
      this.activeTitle.textContent=state.active==='start'?'正在调整 · 起始姿态':'正在调整 · 结束姿态';
      for(const [key,control]of this.endpointButtons){control.setAttribute('aria-pressed',String(key===state.active));this.views.get(key).card.dataset.active=String(key===state.active);}
      for(const control of this.dialog.querySelectorAll('[data-camera-studio-switch]'))control.setAttribute('aria-pressed',String(control.dataset.cameraStudioSwitch===state.active));
      for(const [key,input]of this.inputs)if(document.activeElement!==input){input.value=round(state.poses[state.active][key]);input.setCustomValidity('');}
      this.readout.textContent=`过渡预览 ${Math.round(state.progress*100)}% · X ${round(state.pose.x)} · Y ${round(state.pose.y)} · Z ${round(state.pose.z)}\n偏航 ${round(state.pose.yaw)}° · 俯仰 ${round(state.pose.pitch)}° · 倾斜 ${round(state.pose.roll)}°`;
      this.message.textContent='左侧场景中的两台摄像机对应首尾；下方展示各自实际画面。';this.generateButton.disabled=[...this.inputs.values()].some(input=>!input.checkValidity());this.scene.set(state.poses,state.active);this.requestViews(!views);this.editor.invalidate();
    }catch(error){this.generateButton.disabled=true;this.fail(error);}
  }
  fail(error,focus=false){this.state.playing=false;this.errorAlert.textContent=error.message??String(error);this.errorAlert.hidden=false;if(focus)this.errorAlert.focus({preventScroll:true});}
  requestViews(transitionOnly=false){if(!transitionOnly)this.redrawAll=true;if(this.closed||this.pending)return;this.pending=requestAnimationFrame(()=>{this.pending=0;const all=this.redrawAll;this.redrawAll=false;if(!this.closed)this.drawViews(!all);});}
  drawViews(transitionOnly=false){
    try{
      const r=this.preview,source=this.editor.preview,s=this.state,values=this.values();
      for(const key of ['skin','images','noteSize','lineScale','backgroundAlpha','backgroundBlur','applyShaders','showGameUI','aspectRatio','duration','highlight'])r[key]=source[key];
      Object.assign(r,{visible:true,allLines:true,opacity:1,viewDivisor:1,lineNumbers:false,lineArrows:false,lineTint:false,showHitEffects:false});for(const key of ['hiddenLayers','simpleLayers','forceSimple','seamless'])r.noisePreview[key]=source.noisePreview[key];
      const paint=(canvas,pose,seconds)=>{r.canvas=canvas;r.spatialCameraOverride=pose;r.draw(this.editor.chart,this.editor.tempo,seconds,this.editor.session.lineIndex);if(r.shaderCanvas.style.visibility==='visible'){const c=canvas.getContext('2d');c.setTransform(1,0,0,1,0,0);c.clearRect(0,0,canvas.width,canvas.height);c.drawImage(r.shaderCanvas,0,0);}canvas.style.opacity='1';};
      if(!transitionOnly){paint(this.plane,DEFAULT_POSE,this.editor.tempo.seconds(s.active==='start'?values.start:values.end));this.scene.set(s.poses,s.active,this.plane);for(const key of ['start','end'])paint(this.views.get(key).canvas,s.poses[key],this.editor.tempo.seconds(key==='start'?values.start:values.end));}
      paint(this.transitionCanvas,s.pose,s.seconds);
    }catch(error){this.fail(error);}
  }
  generate(){try{const v=this.values(),changes=transitionChanges(this.editor.globalEvents.document,v.start,v.end,v.first,v.last,v.easingType);this.editor.globalEvents.document.apply(changes,'生成三维摄像机过渡');this.controls.closeAssistant();this.editor.openGlobalEvents();this.api.notify('已生成摄像机过渡：7 列事件，可一步撤销');}catch(error){this.fail(error,true);}}
  dispose(){
    if(this.closed)return;this.closed=true;cancelAnimationFrame(this.pending);this.resize.disconnect();this.scene.dispose();this.releasePreview();
    const r=this.preview;r.overflowLayer?.remove();r.noisePreview.gpu?.clearSkins();r.overflowNoisePreview?.gpu?.clearSkins();for(const gl of [r.noisePreview.gpu?.gl,r.noisePreview.edgeFilter?.gl,r.shaderPipeline.gl,r.overflowNoisePreview?.gpu?.gl,r.overflowNoisePreview?.edgeFilter?.gl,r.overflowShaderPipeline?.gl])gl?.getExtension('WEBGL_lose_context')?.loseContext();r.backgroundFrame.clear();this.listeners.forEach(remove=>remove());
    this.state.unlock();this.state.restoreView();this.controls.stage.closest('.editor').classList.remove('camera3d-adjusting');this.dialog.close();this.unmount();
  }
}
