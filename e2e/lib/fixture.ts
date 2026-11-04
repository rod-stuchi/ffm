import { readFileSync } from 'node:fs';

export const GOLDEN = '/work/golden';
export const FIXTURE = '/work/fixture.json';

export interface Member {
  id: number;
  /// As stored: lowercase. The page shows it capitalized.
  name: string;
  role: number;
  /// A session already in the database, so a test need not sign in.
  sid: string;
}

export interface Fixture {
  seed: string;
  today: string;
  password: string;
  members: Member[];
}

export function fixture(): Fixture {
  return JSON.parse(readFileSync(FIXTURE, 'utf8')) as Fixture;
}
