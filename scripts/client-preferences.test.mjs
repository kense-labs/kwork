import assert from 'node:assert/strict';
import test from 'node:test';
import { createClientPreferences } from '../services/client-preferences.mjs';
import { createClientPreferencesController } from '../ui/client-preferences-controller.ts';

const deferred = () => { let resolve, reject; const promise = new Promise((a,b) => { resolve=a; reject=b; }); return {promise,resolve,reject}; };
const snapshot = (size=14, revision=0) => ({writable:true,namespaces:[
  {ns:'ui-theme',revision,value:{fontSize:size}}, {ns:'ui-settings',revision:0,value:{enabled:true}},
]});

test('Host boundary serializes reads and writes and exposes only preference fields', async () => {
  let size=14, revision=0;
  const gate=deferred(), entered=deferred();
  const settings={writable:true, describe:() => [{ns:'ui-theme',revision,value:{fontSize:size,apiKey:'never expose'}}],
    async mutate(ns,ops,expected) {
      assert.equal(ns,'ui-theme'); entered.resolve(); await gate.promise;
      if(expected!==revision) throw Error('conflict');
      size=ops[0].value;revision++;
    }};
  const call=createClientPreferences({get:()=>settings});
  const write=call({operation:'set',namespace:'ui-theme',revision:0,changes:{fontSize:16}});
  await entered.promise;
  const read=call({operation:'read'}); gate.resolve(); await write;
  assert.deepEqual((await read).namespaces[0],{ns:'ui-theme',revision:1,value:{fontSize:16}});
  await assert.rejects(call({operation:'set',namespace:'ui-theme',revision:0,changes:{fontSize:17}}),/conflict/);
  await assert.rejects(call({operation:'set',namespace:'ui-theme',revision:1,changes:{apiKey:'x'}}),/Invalid/);
  await assert.rejects(call({operation:'set',namespace:'llm-deepseek',revision:0,changes:{fontSize:17}}),/Invalid/);
  for (const namespace of ['constructor','__proto__',['ui-theme']]) {
    await assert.rejects(call({operation:'set',namespace,revision:0,changes:{fontSize:17}}),/Invalid/);
  }
  settings.writable=false;
  await assert.rejects(call({operation:'set',namespace:'ui-theme',revision:1,changes:{fontSize:17}}),/read-only/);
});

test('rapid edits retain optimism until the queue settles, then recover accepted Host values', async () => {
  const calls=[], published=[];
  const controller=createClientPreferencesController(request => {
    const task=deferred();calls.push({request,...task});return task.promise;
  },value=>published.push(value));
  const load=controller.load();calls.shift().resolve(snapshot());await load;
  const first=controller.set('ui-theme',{fontSize:15});
  const second=controller.set('ui-theme',{fontSize:16});
  const failure=assert.rejects(second,/refused/);
  await Promise.resolve();assert.equal(calls[0].request.revision,0);
  calls.shift().resolve(snapshot(15,1));await first;
  assert.equal(published.at(-1).namespaces.some(row=>row.ns==='ui-theme'),false);
  await Promise.resolve();assert.equal(calls[0].request.revision,1);
  calls.shift().reject(Error('refused'));await Promise.resolve();await Promise.resolve();
  assert.equal(calls[0].request.operation,'read');calls.shift().resolve(snapshot(15,1));await failure;
  assert.equal(published.at(-1).namespaces[0].value.fontSize,15);
  const last=controller.set('ui-theme',{fontSize:17});await Promise.resolve();calls.shift().resolve(snapshot(17,2));await last;
  assert.equal(published.at(-1).namespaces[0].value.fontSize,17);
});

test('stale external reads and disconnected writes cannot overwrite the current connection', async () => {
  const calls=[], published=[];
  const controller=createClientPreferencesController(request => {
    const task=deferred();calls.push({request,...task});return task.promise;
  },value=>published.push(value));
  const initial=controller.load();calls.shift().resolve(snapshot());await initial;
  const older=controller.load();const old=calls.shift();
  const write=controller.set('ui-theme',{fontSize:18});await Promise.resolve();
  calls.shift().resolve(snapshot(18,1));await write;old.resolve(snapshot(14,0));await older;
  assert.equal(published.at(-1).namespaces[0].value.fontSize,18);
  const pending=controller.set('ui-settings',{enabled:false});await Promise.resolve();const save=calls.shift();
  const count=published.length;controller.dispose();save.resolve(snapshot());await pending;
  assert.equal(published.length,count);
  await assert.rejects(controller.set('ui-settings',{enabled:true}),/closed/);
});
