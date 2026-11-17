import type { TrajectoryCellProps } from './trajectory-record';
/** The host supplies the unfiltered, unfolded record projection. */
export interface TrajectoryTurnModel {
  turn: number | null;
  groups: readonly { cells: readonly TrajectoryCellProps[] }[];
}
