import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {createServer} from 'node:http';
const source=await readFile(new URL('../services/acp-session-bridge.mjs',import.meta.url),'utf8');
const start=source.indexOf('async function readFileImage(value)');
const end=source.indexOf('/** Resolve one live agent',start);
assert.ok(start>=0&&end>start);
const readImage=new Function(source.slice(start,end)+'\nreturn readFileImage;')();

test('original image carrier follows validated redirects without credentials and preserves bytes',async()=>{
  const seen=[];
  const original=Buffer.from([137,80,78,71,13,10,26,10,0,255]);
  const server=createServer((request,response)=>{
    seen.push(request.headers);
    if(request.url==='/redirect'){response.writeHead(302,{Location:'/image','Set-Cookie':'secret=ignored'});response.end();}
    else if(request.url==='/bad'){response.writeHead(302,{Location:'file:///private/image.png'});response.end();}
    else if(request.url==='/loop'){response.writeHead(302,{Location:'/loop'});response.end();}
    else{response.writeHead(200,{'Content-Type':'image/png'});response.end(original);}
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const base=`http://127.0.0.1:${server.address().port}`;
  try{
    const result=await readImage(base+'/redirect');
    assert.deepEqual(Buffer.from(result.base64,'base64'),original);
    assert.equal(result.mediaType,'image/png');
    assert.ok(seen.every(headers=>!headers.cookie&&!headers.authorization&&!headers.referer));
    await assert.rejects(readImage(base+'/bad'),/Invalid image URL/);
    await assert.rejects(readImage(base+'/loop'),/Invalid image redirect/);
    await assert.rejects(readImage(base.replace('http://','http://user:pass@')+'/image'),/Invalid image URL/);
    await assert.rejects(readImage('file:///private/image.png'),/Invalid image URL/);
  }finally{server.closeAllConnections();await new Promise(resolve=>server.close(resolve));}
});
