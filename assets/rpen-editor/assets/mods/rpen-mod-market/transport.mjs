const aborted=()=>new DOMException('Market stopped','AbortError');
export function marketSocket(url,{WebSocketClass=WebSocket,connectTimeout=15000,requestTimeout=300000}={}){
  let socket=null,opening=null,current=null,serial=Promise.resolve(),csrf='',closed=false,endpoint='';
  const jobs=new Set();
  function refresh(){
    if(closed)throw aborted();
    const next=new URL(url());if(!['ws:','wss:'].includes(next.protocol))throw Error('Invalid market endpoint');
    if(endpoint!==next.href){endpoint=next.href;csrf='';disconnect(aborted());for(const job of jobs)job.finish(aborted());}
    return endpoint;
  }
  function disconnect(error){
    const old=socket;socket=null;
    if(old){old.onopen=old.onclose=old.onerror=old.onmessage=null;old.close();}
    if(opening){const wait=opening;opening=null;clearTimeout(wait.timer);wait.reject(error);}
    current?.finish(error);
  }
  function connect(address){
    if(closed)return Promise.reject(aborted());
    if(socket?.readyState===1)return Promise.resolve(socket);
    if(opening)return opening.promise;
    let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});
    opening={promise,resolve,reject,timer:setTimeout(()=>disconnect(Error('Market connection timed out')),connectTimeout)};
    try{socket=new WebSocketClass(address);}catch(error){disconnect(error);return promise;}
    const connection=socket;connection.binaryType='arraybuffer';
    connection.onopen=()=>{if(socket!==connection)return;const wait=opening;opening=null;clearTimeout(wait.timer);wait.resolve(connection);};
    connection.onerror=()=>{if(socket===connection)disconnect(Error('Market connection rejected'));};
    connection.onclose=()=>{if(socket===connection)disconnect(Error('Market connection closed'));};
    connection.onmessage=event=>{
      if(socket!==connection||!current||current.done)return;
      const job=current;
      try{
        if(event.data instanceof ArrayBuffer){
          if(!job.binary||!job.begun)throw Error('Unexpected binary frame');
          job.parts.push(new Uint8Array(event.data));job.bytes+=event.data.byteLength;
          if(job.bytes>130*1024*1024)throw Error('Market response too large');
          return;
        }
        if(typeof event.data!=='string'||event.data.length>4*1024*1024)throw Error('Invalid market response');
        const message=JSON.parse(event.data);
        if(!message||message.request!==job.id)throw Error('Market response identity mismatch');
        if(message.type==='begin'){
          if(!job.binary||job.begun)throw Error('Unexpected binary header');
          job.begun=true;return;
        }
        if(message.type==='end'){
          if(!job.binary||!job.begun||!job.bytes)throw Error('Incomplete binary response');
          const data=new Uint8Array(job.bytes);let at=0;
          for(const part of job.parts){data.set(part,at);at+=part.length;}
          if(data[0]===123){const result=JSON.parse(new TextDecoder().decode(data));throw Error(result?.error?.code||'Market binary response rejected');}
          job.finish(null,data.buffer);return;
        }
        if(message.type==='json'&&!job.binary&&message.result&&typeof message.result.ok==='boolean'){
          if(message.result.ok){if(!Object.hasOwn(message.result,'data'))throw Error('Missing market response');job.finish(null,message.result.data);}
          else job.finish(Error(message.result.error?.code||'Market request rejected'));
          return;
        }
        throw Error('Market response rejected');
      }catch(error){disconnect(error);}
    };
    return promise;
  }
  function request(action,data,query,{signal,address}={}){
    if(closed||signal?.aborted)return Promise.reject(aborted());
    let target;try{target=refresh();}catch(error){return Promise.reject(error);}if(address&&address!==target)return Promise.reject(aborted());
    let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});
    const job={id:crypto.randomUUID(),binary:action==='download'||action==='cover',begun:false,parts:[],bytes:0,done:false,timer:null,
      finish(error,value){if(job.done)return;job.done=true;clearTimeout(job.timer);signal?.removeEventListener('abort',cancel);jobs.delete(job);job.parts=[];error?reject(error):resolve(value);}};
    const cancel=()=>{if(current===job)disconnect(aborted());else job.finish(aborted());};
    signal?.addEventListener('abort',cancel,{once:true});jobs.add(job);promise.catch(()=>{});
    serial=serial.then(async()=>{
      if(job.done)return;if(closed||signal?.aborted){job.finish(aborted());return;}
      current=job;
      try{
        if(refresh()!==target)throw aborted();
        const connection=await connect(target);
        if(job.done||closed||signal?.aborted){job.finish(aborted());return;}
        job.timer=setTimeout(()=>disconnect(Error('Market request timed out')),requestTimeout);
        connection.send(JSON.stringify({request:job.id,action,query,csrf,...(data===undefined?{}:{data})}));
        await promise;
      }catch(error){if(current===job)disconnect(error);else job.finish(error);}
      finally{if(current===job)current=null;}
    });
    return promise;
  }
  return {
    async json(action,data,query={},options={}){
      const address=refresh();
      if(action==='feedback'&&!csrf){const session=await request('session',undefined,{}, {...options,address});if(refresh()!==address)throw aborted();csrf=session.csrf;}
      return request(action,data,query,{...options,address});
    },
    binary:(action,query,options={})=>request(action,undefined,query,options),
    refresh,endpoint:refresh,
    close(){if(closed)return;closed=true;disconnect(aborted());for(const job of jobs)job.finish(aborted());csrf='';}
  };
}
