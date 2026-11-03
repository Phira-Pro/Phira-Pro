export function pickaxeSprite(){
  const canvas=document.createElement('canvas');canvas.width=20;canvas.height=20;const context=canvas.getContext('2d');
  const pixel=(x,y,color)=>{context.fillStyle=color;context.fillRect(x,y,1,1);};
  // Original twenty-pixel silhouette: diagonal wood handle and a faceted cyan head.
  const shaftHalf=[];for(let step=0;step<12;step++)for(const offset of [-1,0,1])shaftHalf.push([3+step,17-step+offset]);const shaft=[...new Map(shaftHalf.flatMap(([x,y])=>[[x,y],[20-y,20-x]]).map(point=>[point.join(','),point])).values()];
  for(const [x,y]of shaft)for(const [dx,dy]of [[-1,0],[1,0],[0,-1],[0,1]])pixel(x+dx,y+dy,'#241a14');
  for(const [x,y]of shaft)pixel(x,y,(x+y)%2?'#bb8244':'#81532d');
  // Reflect every pixel through x+y=20, the exact centre axis of the handle.
  const half=[[6,3],[7,2],[8,2],[9,2],[10,2],[11,3],[12,3],[13,4],[14,5],[15,6],[14,6],[13,5],[12,5],[11,4],[10,4],[9,4],[8,4],[7,4]];
  const head=[...new Map(half.flatMap(([x,y])=>[[x,y],[20-y,20-x]]).map(point=>[point.join(','),point])).values()];
  for(const [x,y]of head)for(const [dx,dy]of [[-1,0],[1,0],[0,-1],[0,1]])pixel(x+dx,y+dy,'#073b42');
  for(const [x,y]of head)pixel(x,y,Math.min(x+y,40-x-y)<15?'#78fff1':(x-y)%3?'#1adcc9':'#0e9a9f');
  for(const [x,y]of [[8,2],[9,2],[10,2],[12,3]].flatMap(([x,y])=>[[x,y],[20-y,20-x]]))pixel(x,y,'#cefff2');
  return canvas;
}

const label=node=>node.getAttribute('aria-label')||node.getAttribute('title')||node.labels?.[0]?.textContent||node.textContent||node.getAttribute('placeholder')||node.id||node.tagName;
export function itemName(node){return label(node).replace(/\s+/g,' ').trim().slice(0,48)||'控件';}
export function captureAppearance(source){
  const rect=source.getBoundingClientRect();const width=Math.max(12,rect.width),height=Math.max(12,rect.height);let count=0;
  function copy(node){
    if(node.nodeType===Node.TEXT_NODE)return document.createTextNode(node.textContent);
    if(node.nodeType!==Node.ELEMENT_NODE||++count>100)return document.createTextNode('');
    const computed=getComputedStyle(node);let clone;
    if(node instanceof HTMLCanvasElement){const canvas=document.createElement('canvas');const scale=Math.min(1,160/Math.max(node.width,node.height));canvas.width=Math.max(1,Math.round(node.width*scale));canvas.height=Math.max(1,Math.round(node.height*scale));canvas.getContext('2d').drawImage(node,0,0,canvas.width,canvas.height);clone=document.createElement('img');clone.src=canvas.toDataURL();}
    else{clone=node.namespaceURI==='http://www.w3.org/2000/svg'?document.createElementNS(node.namespaceURI,node.localName):document.createElement(node.tagName==='IMG'?'img':node.tagName.toLowerCase());if(node.namespaceURI==='http://www.w3.org/2000/svg')for(const attr of node.attributes)if(!/^on/i.test(attr.name)&&attr.name!=='id')clone.setAttribute(attr.name,attr.value);if(node instanceof HTMLImageElement)clone.src=node.currentSrc||node.src;}
    for(const property of computed)if(!['animation','transition','cursor','content'].some(prefix=>property.startsWith(prefix)))clone.style.setProperty(property,computed.getPropertyValue(property),'important');
    clone.style.setProperty('animation','none','important');clone.style.setProperty('transition','none','important');clone.style.setProperty('pointer-events','none','important');
    if(node instanceof HTMLInputElement){clone.type=node.type==='file'?'text':node.type;clone.value=node.value;clone.checked=node.checked;}
    if(node instanceof HTMLTextAreaElement)clone.value=node.value;
    const pseudo=position=>{if(node.namespaceURI==='http://www.w3.org/2000/svg'||['INPUT','IMG','CANVAS','SELECT','TEXTAREA'].includes(node.tagName))return;const style=getComputedStyle(node,position);if(!style.content||['none','normal'].includes(style.content))return;const part=document.createElement('span');for(const key of style)if(!key.startsWith('animation')&&!key.startsWith('transition'))part.style.setProperty(key,style.getPropertyValue(key),'important');part.style.setProperty('pointer-events','none','important');if(/^['"]/.test(style.content))part.textContent=style.content.slice(1,-1);clone.append(part);};
    pseudo('::before');if(!(node instanceof HTMLCanvasElement))for(const child of node.childNodes)clone.append(copy(child));pseudo('::after');
    if(node instanceof HTMLSelectElement)clone.selectedIndex=node.selectedIndex;
    clone.removeAttribute('id');clone.removeAttribute('name');clone.tabIndex=-1;return clone;
  }
  const template=copy(source);for(const [property,value]of Object.entries({position:'relative',inset:'auto',margin:'0',transform:'none',width:width+'px',height:height+'px','min-width':'0','min-height':'0','max-width':'none','max-height':'none','box-sizing':'border-box',display:getComputedStyle(source).display==='inline'?'inline-block':getComputedStyle(source).display}))template.style.setProperty(property,value,'important');
  const iconSize=42;const scale=Math.min(iconSize/width,iconSize/height,1);
  return{width,height,scale,create(){const host=document.createElement('div');host.className='pm-item-art';host.style.width=width*scale+'px';host.style.height=height*scale+'px';const shadow=host.attachShadow({mode:'open'});const content=document.createElement('div');content.style.cssText=`width:${width}px;height:${height}px;transform:scale(${scale});transform-origin:0 0;pointer-events:none;overflow:hidden`;content.inert=true;content.append(template.cloneNode(true));shadow.append(content);return host;}};
}

export function drawCracks(context,rect,progress){
  context.save();context.beginPath();context.rect(rect.x,rect.y,rect.width,rect.height);context.clip();context.fillStyle=`rgba(15,22,25,${progress*0.16})`;context.fillRect(rect.x,rect.y,rect.width,rect.height);
  const center={x:rect.x+rect.width*0.48,y:rect.y+rect.height*0.48};const branches=3+Math.floor(progress*8);const size=Math.hypot(rect.width,rect.height)*0.56;
  for(let branch=0;branch<branches;branch++){
    const angle=branch*2.399;const length=size*Math.min(1,progress*1.8)*(0.5+(branch%4)*0.15);let x=center.x,y=center.y;
    context.beginPath();context.moveTo(Math.round(x),Math.round(y));for(let step=1;step<=6;step++){x=center.x+Math.cos(angle)*length*step/6+Math.sin(branch*11+step*5)*3;y=center.y+Math.sin(angle)*length*step/6+Math.cos(branch*7+step*3)*3;context.lineTo(Math.round(x/2)*2,Math.round(y/2)*2);if(step===3||step===5){context.moveTo(x,y);context.lineTo(Math.round(x+Math.cos(angle+0.9)*length*0.2),Math.round(y+Math.sin(angle+0.9)*length*0.2));context.moveTo(x,y);}}
    context.strokeStyle='#e8f4df88';context.lineWidth=3;context.stroke();context.strokeStyle='#101516ee';context.lineWidth=1.5;context.stroke();
  }context.restore();
}
