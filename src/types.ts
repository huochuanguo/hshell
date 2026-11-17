export interface Profile {
  id: string;
  name: string;
  host: string;
  username: string;
  port: number | null;
  identityFile: string;
  group: string;
  favorite: boolean;
  passwordSaved?: boolean;
}

export type TerminalEvent =
  | { type: "auth"; request_id: string; prompt: string; confirm: boolean }
  | { type: "exit"; code: number | null; error: string | null };

export interface Environment {
  platform: string;
  sshVersion: string;
  configPath: string;
}
