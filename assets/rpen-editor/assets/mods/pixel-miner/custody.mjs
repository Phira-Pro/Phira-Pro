// Keep real controls in the miner's custody while skins and dynamic panels update.
export function retainControls(api,records,onReplacement){
  api.hook('ui:before-relocate',change=>{
    for(const record of records.values())if(record.node===change.node||record.node.contains(change.node)||change.node.contains(record.node)){
      change.cancel=true;
      if(change.reason==='skin-restore'&&change.anchor?.parentNode){
        if(record.node===change.node){const origin=change.anchor.parentNode;if(record.wrapper?.parentNode===record.marker.parentNode)change.anchor.before(record.wrapper);change.anchor.before(record.marker,record.spacer);record.parent=origin;}
        change.anchor.remove();
      }
      return;
    }
  },100);
  const observer=new MutationObserver(changes=>{
    if(!records.size)return;const nodes=[];
    const selector='[id],input,select,textarea,button,[role=button],[role=checkbox],[role=switch]';
    for(const change of changes)for(const node of change.addedNodes)if(node instanceof Element&&!node.closest('[data-pm-root],[data-pm-placement]')){if(node.matches(selector))nodes.push(node);nodes.push(...node.querySelectorAll(selector));}
    if(!nodes.length)return;
    for(const record of records.values()){
      const identity=record.identity;if(!identity)continue;
      const matches=node=>identity.id?node.id===identity.id:node.localName===identity.tag&&node.getAttribute(identity.attribute)===identity.value&&(identity.scope?.contains(node)||identity.scopeId&&node.closest('#'+CSS.escape(identity.scopeId)));
      const candidate=nodes.find(node=>matches(node)&&node!==record.node&&!record.node.contains(node)&&!node.closest('[data-pm-root],[data-pm-placement]'));
      const replacement=record.node.localName==='label'?candidate?.closest('label'):candidate;
      if(replacement&&replacement!==record.node&&replacement.isConnected)onReplacement(record,replacement);
    }
  });
  observer.observe(document.body,{childList:true,subtree:true});api.dispose(()=>observer.disconnect());
}
