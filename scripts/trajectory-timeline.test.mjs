import { test } from 'bun:test';
import assert from 'node:assert/strict';
import { deriveTrajectoryTimeline, trajectoryTimelineFocusIndexes } from '../ui/vendor/harness-trajectory/timeline.ts';

const cell = (index, startedAt, timeSeconds, kind = 'tool', extra = {}) => ({ index, startedAt, timeSeconds, kind, text: '', ...extra });
const turns = cells => [{ turn: 1, groups: [{ cells }] }];

test('sequence includes untimed operations but never duplicates request boundaries', () => {
  const model = deriveTrajectoryTimeline(turns([
    cell(1, null, null, 'user'), cell(2, 1200, 0.4, 'message'),
    cell(3, 1700, 0.2), cell(4, 2100, null, 'message', { requestOnly: true }),
  ]));
  assert.deepEqual(model.spans.map(({ index, start, end, lane }) => [index, start, end, lane]),
    [[1, 0, 1, 0], [2, 1, 2, 1], [3, 2, 3, 2]]);
  assert.equal(model.end, 3);
});

test('duration removes only idle gaps and preserves overlapping calls', () => {
  const data = turns([cell(1, 1000, 1, 'message'), cell(2, 1300, 0.2), cell(3, 3000, 0.5)]);
  const compressed = deriveTrajectoryTimeline(data, 'duration');
  assert.deepEqual(compressed.spans.map(({ start, end }) => [start, end]), [[1000, 2000], [1300, 1500], [2000, 2500]]);
  const actual = deriveTrajectoryTimeline(data, 'actual');
  assert.deepEqual(actual.spans.map(({ start, end }) => [start, end]), [[1000, 2000], [1300, 1500], [3000, 3500]]);
  assert.deepEqual([...trajectoryTimelineFocusIndexes(data, { start: 2000, end: 2200 }, 'duration')], [1, 3]);
});

test('recorded-time marks use starts, including zero-length operations', () => {
  const data = turns([cell(1, null, null, 'user'), cell(2, 1000, 1), cell(3, 3000, null)]);
  assert.deepEqual(deriveTrajectoryTimeline(data, 'time').spans.map(({ index, start, end }) => [index, start, end]),
    [[2, 1000, 1000], [3, 3000, 3000]]);
  assert.equal(deriveTrajectoryTimeline(turns([cell(1, null, null)]), 'duration'), null);
});

test('turn boundaries use the same projection as record selection', () => {
  const data = [{ turn: 1, groups: [{ cells: [cell(1, 1000, 0)] }] },
    { turn: 2, groups: [{ cells: [cell(2, 2000, 0.3, 'message')] }] }];
  assert.deepEqual(deriveTrajectoryTimeline(data).turnBoundaries, [{ turn: 1, time: 0 }, { turn: 2, time: 1 }]);
  assert.deepEqual([...trajectoryTimelineFocusIndexes(data, { start: 1.1, end: 1.9 })], [2]);
});
