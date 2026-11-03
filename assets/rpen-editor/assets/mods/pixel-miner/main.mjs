import {isTextEntry} from '@rpen/ui/keyboard.mjs';
import {createInventory,reserveItem,takeItem,experienceLevel,dropPose,advanceExperienceOrb,PICKUP_DELAY} from './model.mjs';
import {pickaxeSprite,captureAppearance,itemName,drawCracks} from './pixels.mjs';
import {controlAt,extractControl,stashControl,placementBounds,placeControl,restoreControl,discardControl} from './controls.mjs';
import {createSounds} from './sound.mjs';
import {retainControls} from './custody.mjs';

const element=(tag,className,text)=>{const node=document.createElement(tag);node.className=className;if(text)node.textContent=text;return node;};
export function activate(api){
  api.style(api.package.text('style.css'));
  const root=element('div','pm-root');root.id='pixel-miner-ui';root.dataset.pmRoot='true';root.dataset.modInteractive='true';root.setAttribute('popover','manual');
  const effects=element('canvas','pm-effects');effects.setAttribute('aria-hidden','true');const context=effects.getContext('2d');
  const hotbar=element('div','pm-hotbar');hotbar.setAttribute('role','toolbar');hotbar.setAttribute('aria-label','像素矿工物品栏，数字 1 到 9 选择');
  const xp=element('div','pm-xp');const level=element('span','pm-level','0');const track=element('div','pm-xp-track');const fill=element('i','pm-xp-fill');track.append(fill);xp.append(level,track);
  const name=element('div','pm-item-name');name.setAttribute('role','status');const hand=element('div','pm-hand');hand.setAttribute('aria-hidden','true');const stash=element('div','pm-stash');stash.hidden=true;stash.inert=true;
  root.append(effects,hotbar,xp,name,hand,stash);api.mount(root);root.showPopover();
  const pickaxe=pickaxeSprite();const pickUrl=pickaxe.toDataURL();const inventory=createInventory();const records=new Map();const drops=new Set();const particles=[];const orbs=[];const slots=[];const sounds=createSounds(api.editor,()=>api.settings.get('sound-volume'));
  let enabled=false,mining=null,hover=null,mouse={x:innerWidth/2,y:innerHeight/2,inside:false},now=performance.now()/1000,tooltipUntil=0,lastChip=0,swallowClick=false,id=0,lastWidth=0,lastHeight=0,painted=false;
  function raise(){root.hidePopover();root.showPopover();}
  function selectedRecord(){return records.get(inventory.slots[inventory.selected]);}
  function updateTool(){const item=inventory.slots[inventory.selected];if(enabled&&(item==='pickaxe'||item))document.documentElement.dataset.pmTool=item==='pickaxe'?'pickaxe':'item';else delete document.documentElement.dataset.pmTool;}
  function showName(){name.textContent=inventory.selected===0?'钻石镐':selectedRecord()?.name??'空栏位';tooltipUntil=now+2.6;name.classList.toggle('visible',enabled);}
  function refresh(){
    for(let index=0;index<slots.length;index++){const button=slots[index];button.replaceChildren();const key=element('small','',String(index+1));button.append(key);const item=inventory.slots[index];if(item==='pickaxe'){const icon=new Image();icon.src=pickUrl;icon.className='pm-pick-icon';icon.alt='钻石镐';button.append(icon);}else{const record=records.get(item);if(record?.phase==='inventory')button.append(record.art.create());}button.setAttribute('aria-pressed',String(index===inventory.selected));button.title=index===0?'钻石镐':records.get(item)?.name??'空栏位';}
    hand.replaceChildren();const held=selectedRecord();if(held?.phase==='inventory')hand.append(held.art.create());updateTool();
  }
  function select(index){mining=null;inventory.selected=index;refresh();showName();}
  retainControls(api,records,(record,replacement)=>{
    if(mining?.target===record.node)mining=null;if(hover===record.node)hover=null;
    const bounds=record.wrapper?.getBoundingClientRect(),point=bounds?.width?{x:bounds.left+bounds.width/2,y:bounds.top+bounds.height/2}:record.placement,phase=record.phase,visible=replacement.getBoundingClientRect().width>0,art=visible?captureAppearance(replacement):record.art,name=itemName(replacement);discardControl(record);
    Object.assign(record,extractControl(replacement,record.id,stash,record),{phase,art,name});
    if(phase==='placed'){placeControl(record,point,document.body);raise();}
    else if(record.dropElement)record.dropElement.replaceChildren(art.create());
    refresh();
  });
  for(let index=0;index<9;index++){const button=element('button','pm-slot');button.type='button';button.dataset.pmSlot=String(index);button.setAttribute('aria-label','物品栏 '+(index+1));button.onclick=()=>select(index);hotbar.append(button);slots.push(button);}
  function toggle(){enabled=!enabled;mining=null;hover=null;hotbar.hidden=xp.hidden=!enabled;hand.hidden=!enabled;updateTool();if(enabled){raise();showName();}else name.classList.remove('visible');}
  function restoreAll(){
    mining=null;hover=null;swallowClick=false;for(const record of [...records.values()].reverse()){record.dropElement?.remove();restoreControl(record);}records.clear();drops.clear();particles.length=0;orbs.length=0;Object.assign(inventory,createInventory());level.textContent='0';fill.style.width='0%';refresh();if(enabled){name.textContent='控件已全部复原';tooltipUntil=now+2.6;name.classList.add('visible');}api.editor.invalidate();
  }
  function releaseDrop(record){if(record.dropElement)return;const visual=element('div','pm-drop');visual.dataset.pmDrop=record.id;visual.append(record.art.create());root.append(visual);record.dropElement=visual;drops.add(record);}
  function breakControl(target){
    const rect=target.getBoundingClientRect();if(!rect.width||!rect.height)return;const art=captureAppearance(target),name=itemName(target);let record=[...records.values()].find(item=>item.node===target);
    if(record)stashControl(record,stash);else{record=extractControl(target,'item-'+(++id),stash);records.set(record.id,record);}
    Object.assign(record,{name,art,phase:'drop',width:rect.width,height:rect.height,birth:now,x:rect.left+rect.width/2,y:rect.top+rect.height/2,drift:(Math.random()-0.5)*180,initialScale:1/art.scale});releaseDrop(record);sounds.play('break');
    for(let index=0;index<18;index++)particles.push({x:record.x,y:record.y,vx:(Math.random()-0.5)*250,vy:-80-Math.random()*170,birth:now,color:index%3?'#8e9790':'#54e9d3'});
    for(let index=0;index<3;index++)orbs.push({x:record.x,y:record.y,vx:(Math.random()-0.5)*140,vy:-100-Math.random()*90,birth:now-index*0.06});
    hover=null;mining=null;api.editor.invalidate();
  }
  function heldPlacement(record,point){return placementBounds(record,{x:point.x+14+record.art.width*record.art.scale/2,y:point.y+16+record.art.height*record.art.scale/2});}
  function place(point){const record=selectedRecord();if(!record||record.phase!=='inventory')return;placeControl(record,heldPlacement(record,point),document.body);takeItem(inventory);record.phase='placed';raise();sounds.play('place');refresh();showName();api.editor.invalidate();}
  function cancelMine(){mining=null;}
  function input(payload){
    const event=payload.event;if(event.__pmForwarded)return;if(event.type==='pointerdown')swallowClick=false;if(!enabled){if(swallowClick&&['pointerup','mouseup','click'].includes(event.type)&&event.button===0){if(event.type==='click')swallowClick=false;payload.handled=true;}return;}
    if(event.target?.closest?.('#mod-manager[open],#mod-import-warning[open]'))return;
    if(event.type==='keydown'&&!event.ctrlKey&&!event.metaKey&&!event.altKey&&!event.shiftKey&&!event.isComposing&&!isTextEntry(event.target)&&/^[1-9]$/.test(event.key)){if(!event.repeat)select(Number(event.key)-1);payload.handled=true;return;}
    if(event.type==='keyup'&&!event.ctrlKey&&!event.metaKey&&!event.altKey&&!event.shiftKey&&!isTextEntry(event.target)&&/^[1-9]$/.test(event.key)){payload.handled=true;return;}
    if(event.target?.closest?.('#mods-open,#mods-status,#mod-manager,#mod-import-warning'))return;
    const placed=event.target?.closest?.('[data-pm-placement]');if(event.target?.closest?.('[data-pm-root]')&&!placed)return;
    if(swallowClick&&['pointerup','mouseup','mousedown','click'].includes(event.type)&&event.button===0){if(event.type==='pointerup'||event.type==='mouseup')cancelMine();if(event.type==='click')swallowClick=false;payload.handled=true;return;}
    const item=inventory.slots[inventory.selected];if(!item)return;
    if(['pointerdown','mousedown','mouseup','click','dblclick','contextmenu','dragstart'].includes(event.type)&&event.button===0){
      if(event.type==='pointerdown'){
        swallowClick=true;mouse={x:event.clientX,y:event.clientY,inside:true};
        if(item==='pickaxe'){const target=controlAt(event.target,records);if(target){const rect=target.getBoundingClientRect();mining={target,start:now,rect:{x:rect.left,y:rect.top,width:rect.width,height:rect.height}};lastChip=now-1;sounds.play('chip');}}else place(mouse);
      }
      payload.handled=true;
    }
    if(event.type==='pointerup'&&event.button===0){cancelMine();payload.handled=true;}
    if(event.type==='click'&&swallowClick){swallowClick=false;payload.handled=true;}
  }
  api.hook('input:before',input,50);
  api.listen(window,'pointermove',event=>{mouse={x:event.clientX,y:event.clientY,inside:true};hover=enabled&&inventory.selected===0?controlAt(event.target,records):null;if(mining){const rect=mining.target.getBoundingClientRect();if(!mining.target.isConnected||mouse.x<rect.left||mouse.x>rect.right||mouse.y<rect.top||mouse.y>rect.bottom)cancelMine();}},true);
  api.listen(window,'pointerup',cancelMine,true);api.listen(window,'pointercancel',cancelMine,true);api.listen(window,'blur',()=>{mouse.inside=false;cancelMine();});api.listen(document,'pointerout',event=>{if(!event.relatedTarget){mouse.inside=false;cancelMine();}});
  api.listen(window,'wheel',cancelMine,{capture:true,passive:true});
  api.listen(window,'resize',()=>{cancelMine();for(const record of records.values())if(record.wrapper){const rect=record.wrapper.getBoundingClientRect();const left=Math.max(4,Math.min(innerWidth-rect.width-4,rect.left)),top=Math.max(4,Math.min(innerHeight-rect.height-4,rect.top));record.wrapper.style.left=left+'px';record.wrapper.style.top=top+'px';record.placement={x:left+rect.width/2,y:top+rect.height/2};}});
  api.commands.register({id:'toggle',title:'开启 / 关闭像素矿工',setting:'toggle-key',allowInDialog:true,run:toggle});
  api.commands.register({id:'restore',title:'恢复全部控件',setting:'restore-key',allowInDialog:true,run:restoreAll});
  function collect(record,index){record.phase='collecting';record.slot=index;record.collectStart=now;const pose=dropPose(record,now,innerWidth,innerHeight);record.collectFrom=pose;sounds.play('pickup');refresh();}
  api.onFrame(({timestamp,elapsed})=>{
    now=timestamp/1000;if(!enabled&&!drops.size&&!orbs.length&&!particles.length){if(painted){context.resetTransform();context.clearRect(0,0,effects.width,effects.height);painted=false;}return;}painted=true;
    const width=innerWidth,height=innerHeight,ratio=devicePixelRatio||1;if(width!==lastWidth||height!==lastHeight||effects.width!==Math.round(width*ratio)){lastWidth=width;lastHeight=height;effects.width=Math.round(width*ratio);effects.height=Math.round(height*ratio);}context.setTransform(ratio,0,0,ratio,0,0);context.clearRect(0,0,width,height);context.imageSmoothingEnabled=false;
    if(mining){const progress=Math.min(1,(now-mining.start)/api.settings.get('mine-duration'));if(!mining.target.isConnected)cancelMine();else{drawCracks(context,mining.rect,progress);if(now-lastChip>0.16){lastChip=now;sounds.play('chip');}if(progress>=1)breakControl(mining.target);}}
    else if(enabled&&inventory.selected===0&&hover?.isConnected){const rect=hover.getBoundingClientRect();context.strokeStyle='#8effd2aa';context.lineWidth=1;context.strokeRect(rect.left+0.5,rect.top+0.5,Math.max(0,rect.width-1),Math.max(0,rect.height-1));}
    for(const record of drops){
      const pose=dropPose(record,now,width,height);let {x,y}=pose,scale=pose.scale;let turn=pose.rotation;
      if(record.phase==='drop'&&now-record.birth>PICKUP_DELAY){const index=reserveItem(inventory,record.id);if(index>0)collect(record,index);}
      if(record.phase==='collecting'){const t=Math.min(1,(now-record.collectStart)/0.42),amount=t*t*(3-2*t);const bounds=slots[record.slot].getBoundingClientRect();const targetX=bounds.width?bounds.left+bounds.width/2:width/2+(record.slot-4)*50;const targetY=bounds.height?bounds.top+bounds.height/2:height-38;x=record.collectFrom.x+(targetX-record.collectFrom.x)*amount;y=record.collectFrom.y+(targetY-record.collectFrom.y)*amount;scale=record.collectFrom.scale+(1-record.collectFrom.scale)*amount;turn=0;if(t>=1){record.phase='inventory';record.dropElement.remove();record.dropElement=null;drops.delete(record);refresh();if(inventory.selected===record.slot)showName();continue;}}
      record.dropElement.style.transform=`translate(${x}px,${y}px) translate(-50%,-50%) perspective(350px) rotateY(${turn}deg) rotateZ(-7deg) scale(${scale})`;
    }
    const dt=Math.max(0,Math.min(0.05,elapsed));for(let index=orbs.length-1;index>=0;index--){const orb=orbs[index];if(now<orb.birth)continue;const target=mouse.inside?mouse:{x:width/2,y:height-85};if(advanceExperienceOrb(orb,now,dt,target,width,height)){orbs.splice(index,1);inventory.experience++;const value=experienceLevel(inventory.experience);level.textContent=String(value.level);fill.style.width=value.progress*100+'%';sounds.play('xp');continue;}const x=Math.round(orb.x),y=Math.round(orb.y);context.fillStyle='#153b08';context.fillRect(x-3,y-4,6,9);context.fillRect(x-4,y-3,9,6);context.fillStyle='#76ef28';context.fillRect(x-3,y-3,6,6);context.fillStyle='#d2ff75';context.fillRect(x-2,y-2,3,3);}
    for(let index=particles.length-1;index>=0;index--){const p=particles[index],age=now-p.birth;if(age>0.7){particles.splice(index,1);continue;}context.globalAlpha=1-age/0.7;context.fillStyle=p.color;context.fillRect(p.x+p.vx*age,p.y+p.vy*age+age*age*280,4,4);}context.globalAlpha=1;
    const held=selectedRecord();hand.hidden=!enabled||!mouse.inside||held?.phase!=='inventory';if(!hand.hidden){const center=heldPlacement(held,mouse);hand.style.transform=`translate(${center.x}px,${center.y}px) translate(-50%,-50%) scale(1.35)`;}
    if(enabled&&mouse.inside&&inventory.selected===0){const swing=mining?Math.sin((now-mining.start)*23)*0.38:0;context.save();context.translate(mouse.x+28,mouse.y+32);context.rotate(swing-0.12);context.shadowColor='#000a';context.shadowBlur=2;context.drawImage(pickaxe,-35,-35,76,76);context.restore();}
    if(now>tooltipUntil)name.classList.remove('visible');
  });
  refresh();hotbar.hidden=xp.hidden=hand.hidden=true;fill.style.width='0%';
  api.dispose(()=>{enabled=false;restoreAll();delete document.documentElement.dataset.pmTool;sounds.dispose();});
}
