// Mutable payloads intentionally allow mods to change editor semantics and custom formats.
const hooks=new Map();
export function registerModHook(name,callback,priority=0,onError=console.error){
  const entry={callback,priority,onError};if(!hooks.has(name))hooks.set(name,[]);hooks.get(name).push(entry);hooks.get(name).sort((a,b)=>b.priority-a.priority);
  return()=>{const list=hooks.get(name);const index=list?.indexOf(entry)??-1;if(index>=0)list.splice(index,1);};
}
export function emitModHook(name,payload){for(const entry of [...(hooks.get(name)??[])])try{entry.callback(payload);}catch(error){entry.onError(error);}return payload;}
