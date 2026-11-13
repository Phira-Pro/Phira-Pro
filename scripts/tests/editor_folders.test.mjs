import test from 'node:test';
import assert from 'node:assert/strict';
import {directoryFiles,directoryHandle} from '../../assets/rpen-editor/pro/folders.mjs';
import {writeZip} from '../../assets/rpen-editor/src/platform/archive.mjs';

test('directory transport preserves nested Unicode paths and independent bytes',async()=>{
 const blob=writeZip(new Map([['示例/manifest.json',new TextEncoder().encode('{}')],['示例/nested/main.mjs',new Uint8Array([1,2,3])]]));
 const files=await directoryFiles(await blob.arrayBuffer());
 assert.deepEqual(files.map(f=>[f.name,f.webkitRelativePath]),[['manifest.json','示例/manifest.json'],['main.mjs','示例/nested/main.mjs']]);
 assert.deepEqual([...new Uint8Array(await files[1].arrayBuffer())],[1,2,3]);
 const handle=directoryHandle(files);
 assert.equal(handle.name,'示例');
 const nested=[...handle.values()].find(e=>e.name==='nested');
 const clone=await [...nested.values()][0].getFile();
 Object.defineProperty(clone,'webkitRelativePath',{value:'nested/main.mjs'});
 assert.deepEqual([...new Uint8Array(await clone.arrayBuffer())],[1,2,3]);
});
test('directory transport rejects missing root, traversal and empty folders',async()=>{
 await assert.rejects(directoryFiles(await writeZip(new Map()).arrayBuffer()),/没有文件/);
 await assert.rejects(directoryFiles(await writeZip(new Map([['rootless.json',new Uint8Array()]])).arrayBuffer()),/路径/);
 await assert.rejects(directoryFiles(await writeZip(new Map([['root/./bad',new Uint8Array()]])).arrayBuffer()),/路径/);
});
test('iOS ZIP fallback accepts both Files archives and archives without a wrapper folder',async()=>{
 const bytes=new Uint8Array([1,2]);
 const wrapped=writeZip(new Map([['曲素材/manifest.json',bytes],['曲素材/sounds/song.ogg',bytes],['__MACOSX/._曲素材',bytes],['曲素材/.DS_Store',bytes]]));
 assert.deepEqual((await directoryFiles(await wrapped.arrayBuffer(),'archive.zip')).map(file=>file.webkitRelativePath),
   ['曲素材/manifest.json','曲素材/sounds/song.ogg']);
 const flat=writeZip(new Map([['manifest.json',bytes],['sounds/song.ogg',bytes]]));
 assert.deepEqual((await directoryFiles(await flat.arrayBuffer(),'曲素材.zip')).map(file=>file.webkitRelativePath),
   ['曲素材/manifest.json','曲素材/sounds/song.ogg']);
});
test('directory handles re-read selected media instead of replaying the first snapshot',async()=>{
 const encode=new TextEncoder();
 const snapshot=async text=>directoryFiles(await writeZip(new Map([['root/skin.json',encode.encode(text)]])).arrayBuffer());
 let reads=0;
 const handle=directoryHandle(await snapshot('before'),async()=>{reads++;return snapshot('after');});
 const read=async()=>{for await(const file of handle.values())return (await file.getFile()).text();};
 assert.equal(await read(),'before');
 assert.equal(await read(),'after');
 assert.equal(reads,1);
});
