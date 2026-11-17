import assert from 'node:assert/strict';
import test from 'node:test';
import { createClientPreferencesController } from '../ui/client-preferences-controller.ts';

test('coding-view writes stay ordered across row lifetimes and recover after refusal', async () => {
  const pending = [], accepted = [];
  let enabled = true, revision = 0;
  const snapshot = () => ({writable:true,namespaces:[{ns:'ui-settings',revision,value:{enabled}}]});
  const owner = createClientPreferencesController(request => request.operation === 'read' ? Promise.resolve(snapshot()) : new Promise((resolve,reject) => {
    pending.push({enabled:request.changes.enabled,reject,resolve:value => {
      assert.equal(request.revision,revision);enabled=value;revision++;resolve(snapshot());
    }});
  }), value => {
    const next=String(value.namespaces[0].value.enabled);
    if (accepted.at(-1)!==next) accepted.push(next);
  });
  await owner.load();accepted.length=0;
  const write = enabled => owner.set('ui-settings',{enabled});
  const first = write(false), second = write(true);
  await Promise.resolve();
  assert.equal(pending.length, 1, 'successor waits for the current native call');
  pending.shift().resolve(false);
  await first; await Promise.resolve();
  assert.equal(pending[0].enabled, true);
  pending.shift().resolve(true); await second;
  assert.deepEqual(accepted, ['false', 'true']);

  const failed = write(false), next = write(true);
  const refusal = assert.rejects(failed, /refused/);
  await Promise.resolve(); pending.shift().reject(new Error('refused'));
  await refusal; await Promise.resolve();
  assert.equal(pending[0].enabled, true);
  pending.shift().resolve(true); await next;
  assert.deepEqual(accepted, ['false', 'true'], 'refusal leaves accepted state unchanged');
});
