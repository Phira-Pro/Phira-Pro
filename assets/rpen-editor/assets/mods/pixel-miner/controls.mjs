const CONTROL='button,input:not([type=hidden]),select,textarea,summary,a[href],[role=button],[role=checkbox],[role=switch],canvas';
const BLOCKED='#mod-manager,#mod-import-warning,#mods-open,#mods-status,rpen-skin-recovery,[data-pm-root]';
const generatedForms=new WeakMap();

export function controlAt(target,records){
  if(!(target instanceof Element))return null;const placed=target.closest('[data-pm-placement]');if(placed)return records.get(placed.dataset.pmPlacement)?.node??null;
  if(target.closest(BLOCKED))return null;
  const label=target.closest('label');const toggle=label?.control;
  const node=toggle&&['checkbox','radio'].includes(toggle.type)?label.contains(toggle)?label:toggle:target.closest(CONTROL);if(!node||node===document.body||!node.isConnected||node.closest('[data-pm-placeholder]'))return null;
  if(node instanceof HTMLInputElement&&node.type==='file')return null;
  const control=node.localName==='summary'&&node.parentElement?.localName==='details'?node.parentElement:node;
  return control.getBoundingClientRect().width>0?control:null;
}

function placeholder(node,rect){
  const style=getComputedStyle(node);const spacer=document.createElement('span');spacer.dataset.pmPlaceholder='true';spacer.setAttribute('aria-hidden','true');spacer.inert=true;
  for(const property of ['display','box-sizing','margin-top','margin-right','margin-bottom','margin-left','flex-grow','flex-shrink','flex-basis','align-self','justify-self','grid-area','order','vertical-align'])spacer.style.setProperty(property,style.getPropertyValue(property));
  Object.assign(spacer.style,{display:style.display==='inline'?'inline-block':style.display,width:rect.width+'px',height:rect.height+'px',minWidth:rect.width+'px',minHeight:rect.height+'px',visibility:'hidden',pointerEvents:'none'});return spacer;
}

export function extractControl(node,id,stash,fallback={}){
  const measured=node.getBoundingClientRect(),rect={width:measured.width||fallback.width||12,height:measured.height||fallback.height||12};const marker=document.createComment('pixel miner original control');const spacer=placeholder(node,rect);const parent=node.parentNode;
  const computed=getComputedStyle(node);const appearance=Object.fromEntries(['font','color','background-color','border','border-radius','padding'].map(key=>[key,computed.getPropertyValue(key)]));
  const formBindings=[node,...node.querySelectorAll('input,select,textarea,button,fieldset,output')].filter(field=>field.form&&field.localName!=='label').map(field=>({node:field,form:field.form,original:field.getAttribute('form')}));
  const field=node.localName==='label'?node.control:node;const fieldId=node.id||field?.id;const attribute=['data-rpen-field','data-key','name','aria-label'].find(key=>field?.getAttribute(key));const scope=field?.parentElement?.closest('form[id],section[data-panel],[role=dialog]')??field?.parentElement;
  const identity=fieldId?{id:fieldId}:attribute?{attribute,value:field.getAttribute(attribute),tag:field.localName,scope,scopeId:scope?.id}:null;
  const record={id,node,parent,marker,spacer,identity,appearance,formBindings,formLeases:new Set(),originalStyle:node.getAttribute('style'),originalForm:node.getAttribute('form'),form:node.form??null,width:rect.width,height:rect.height,wrapper:null,bridge:null};
  node.before(marker,spacer);stash.append(node);return record;
}

function restoreAttributes(record){
  const {node}=record;if(record.originalStyle===null)node.removeAttribute('style');else node.setAttribute('style',record.originalStyle);if(record.originalForm===null)node.removeAttribute('form');else node.setAttribute('form',record.originalForm);
  for(const binding of record.formBindings??[])if(binding.original===null)binding.node.removeAttribute('form');else binding.node.setAttribute('form',binding.original);
  for(const form of record.formLeases??[]){const state=generatedForms.get(form);state?.users.delete(record);if(state&&!state.users.size){if(form.id===state.id)form.removeAttribute('id');generatedForms.delete(form);}}record.formLeases?.clear();
}

export function stashControl(record,stash){
  if(record.wrapper){if(record.wrapper.matches(':popover-open'))record.wrapper.hidePopover();stash.append(record.node);record.bridge?.();record.bridge=null;record.wrapper.remove();record.wrapper=null;}else stash.append(record.node);
  restoreAttributes(record);
}

export function placementBounds(record,point,viewportWidth=innerWidth,viewportHeight=innerHeight){
  const width=Math.min(record.width,Math.max(30,viewportWidth-12)),height=Math.min(record.height,Math.max(20,viewportHeight-12));
  const left=Math.max(4,Math.min(viewportWidth-width-4,point.x-width/2)),top=Math.max(4,Math.min(viewportHeight-height-4,point.y-height/2));
  return{left,top,width,height,x:left+width/2,y:top+height/2};
}

export function placeControl(record,point,portal){
  const {node}=record;const wrapper=document.createElement('div');wrapper.dataset.pmPlacement=record.id;wrapper.setAttribute('popover','manual');wrapper.className='pm-placement';
  const {left,top,width,height}=placementBounds(record,point);
  wrapper.style.cssText=`position:fixed;inset:auto;margin:0;padding:0;border:0;background:transparent;overflow:visible;box-sizing:border-box;pointer-events:auto;display:flex;align-items:center;justify-content:center;left:${left}px;top:${top}px;width:${width}px;height:${height}px;max-width:none;max-height:none;`;
  const parent=record.marker.parentNode??record.parent;
  for(const {node:field,form}of record.formBindings??[]){if(!form.id){form.id='pm-form-'+crypto.randomUUID();generatedForms.set(form,{id:form.id,users:new Set()});}const lease=generatedForms.get(form);if(lease?.id===form.id){lease.users.add(record);record.formLeases.add(form);}field.setAttribute('form',form.id);}
  for(const [key,value]of Object.entries({width:width+'px',height:height+'px','min-width':'0','min-height':'0','max-width':'none','max-height':'none',margin:'0',position:'relative',inset:'auto',transform:'none','box-sizing':'border-box'}))node.style.setProperty(key,value,'important');
  wrapper.append(node);if(record.marker.parentNode)record.marker.before(wrapper);else portal.append(wrapper);wrapper.showPopover();
  if(!wrapper.getBoundingClientRect().width){
    // A closed panel can hide a top-layer descendant. Keep the live node visible in a portal,
    // forwarding delegated field events to its original form or container.
    wrapper.hidePopover();portal.append(wrapper);wrapper.showPopover();
    for(const [key,value]of Object.entries(record.appearance))if(value)node.style.setProperty(key,value,'important');
    const forward=event=>{if(event.__pmForwarded)return;const copy=new event.constructor(event.type,event);Object.defineProperty(copy,'target',{value:event.target});Object.defineProperty(copy,'composedPath',{value:()=>[event.target,parent,...event.composedPath().filter(item=>item!==wrapper)]});Object.defineProperty(copy,'__pmForwarded',{value:true});parent.dispatchEvent(copy);event.stopPropagation();};
    for(const type of ['input','change','click'])wrapper.addEventListener(type,forward);record.bridge=()=>{for(const type of ['input','change','click'])wrapper.removeEventListener(type,forward);};
  }
  record.wrapper=wrapper;record.placement={x:left+width/2,y:top+height/2};return wrapper;
}

export function restoreControl(record){
  const {node,marker,spacer}=record;
  if(record.wrapper?.matches(':popover-open'))record.wrapper.hidePopover();record.bridge?.();
  if(marker.parentNode)marker.before(node);else{const replacement=node.id&&[...record.parent.querySelectorAll('[id]')].find(item=>item!==node&&item.id===node.id);if(replacement)node.remove();else record.parent.append(node);}
  record.wrapper?.remove();record.wrapper=null;restoreAttributes(record);spacer.remove();marker.remove();
}

export function discardControl(record){if(record.wrapper?.matches(':popover-open'))record.wrapper.hidePopover();record.bridge?.();record.wrapper?.remove();restoreAttributes(record);record.node.remove();record.marker.remove();record.spacer.remove();}
