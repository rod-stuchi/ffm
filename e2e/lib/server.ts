import { type ChildProcess, execFileSync, spawn } from 'node:child_process';
import { setTimeout as sleep } from 'node:timers/promises';

export const FFM = '/cache/target/debug/ffm';
export const ANON = '/cache/target/debug/ffm-anon';
/// Where the server finds `static/`: it reads that relative to where it runs.
export const SOURCE = '/src';
export const TIMEZONE = 'America/Sao_Paulo';

/// Reads with the sqlite3 CLI: no native module to match to the image's Node.
export function query<T>(db: string, sql: string): T[] {
  const out = execFileSync('sqlite3', ['-readonly', '-json', db, sql], { encoding: 'utf8' });
  return out.trim() ? (JSON.parse(out) as T[]) : [];
}

export function write(db: string, sql: string): void {
  execFileSync('sqlite3', ['-bail', db], { input: sql });
}

export interface Paths {
  db: string;
  vault: string;
  backups: string;
}

export class Server {
  log = '';
  readonly url: string;
  private exited: Promise<void>;

  private constructor(
    private child: ChildProcess,
    port: number,
  ) {
    this.url = `http://127.0.0.1:${port}`;
    child.stdout?.on('data', (d) => (this.log += d));
    child.stderr?.on('data', (d) => (this.log += d));
    this.exited = new Promise((done) => child.once('exit', () => done()));
  }

  static async start(paths: Paths, port: number, today: string): Promise<Server> {
    const child = spawn(FFM, [], {
      cwd: SOURCE,
      stdio: ['ignore', 'pipe', 'pipe'],
      env: {
        ...process.env,
        FFM_DB_PATH: paths.db,
        FFM_RECEIPTS_ROOT: paths.vault,
        FFM_BACKUP_DIR: paths.backups,
        FFM_ADDR: `127.0.0.1:${port}`,
        FFM_TODAY: today,
        TZ: TIMEZONE,
      },
    });
    const server = new Server(child, port);
    await server.answering();
    return server;
  }

  private async answering(): Promise<void> {
    let gone = false;
    void this.exited.then(() => (gone = true));
    for (let i = 0; i < 600; i++) {
      if (gone) throw new Error(`ffm exited while starting:\n${this.log}`);
      try {
        if ((await fetch(`${this.url}/login`)).ok) return;
      } catch {
        // Not bound yet.
      }
      await sleep(50);
    }
    throw new Error(`ffm did not answer in 30s:\n${this.log}`);
  }

  /// The server answers before its materializer and upkeep finish, so a test
  /// that depends on what they write waits for the log and the rows to settle.
  async settled(db: string): Promise<void> {
    const sample = () =>
      JSON.stringify(
        query(
          db,
          `SELECT (SELECT count(*) FROM change) c, (SELECT count(*) FROM transactions) t,
                  (SELECT count(*) FROM receipts) r`,
        ),
      );
    let last = sample();
    let same = 0;
    for (let i = 0; i < 100 && same < 3; i++) {
      await sleep(250);
      const now = sample();
      same = now === last ? same + 1 : 0;
      last = now;
    }
  }

  /// SIGTERM, which the server answers by checkpointing the WAL.
  async stop(): Promise<void> {
    this.child.kill('SIGTERM');
    await this.exited;
  }
}
