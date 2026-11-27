import { describe, expect, it } from 'vitest';
import { isLikelyTerminalSession, parseTerminalSession } from './terminalSessionParser';

describe('terminalSessionParser', () => {
  it('reconstructs WKH-02 Kali zsh session with ANSI colors, OSC title, backspace and CSI cursor overwrite', () => {
    const raw =
      'Script started on 2026-08-06 03:12:58+00:00 [TERM="xterm-256color" TTY="/dev/pts/1" COLUMNS="77" LINES="28"]\r\n' +
      '\x1b]0;root@kali-raspberrypi: /home/kali\x07' +
      '\x1b[0m\x1b[27m\x1b[24m\x1b[J' +
      '\x1b[34m┌──(\x1b[1m\x1b[34m\x1b[31mroot㉿kali-raspberrypi\x1b[0m\x1b[31m\x1b[34m)-[\x1b[1m\x1b[34m\x1b[39m/home/kali\x1b[0m\x1b[34m]\r\n' +
      '└─\x1b[1m\x1b[34m\x1b[31m#\x1b[0m\x1b[31m\x1b[39m \x1b[K\x1b[?1h\x1b=\x1b[?2004hl\x08\x1b[36ml\x1b[39m\x1b[38;5;244ms\x1b[39m\x08\x1b[36ml\x08\x1b[36ms\x1b[39m\x1b[?1l\x1b>\x1b[?2004l\r\n' +
      '\x1b[0m\x1b[01;34mDesktop\x1b[0m  \x1b[0m\x1b[01;34mMusic\x1b[0m  wlan1mon.pcap\r\n' +
      '\x1b]0;root@kali-raspberrypi: /home/kali\x07' +
      '\x1b[34m┌──(\x1b[31mroot㉿kali-raspberrypi\x1b[34m)-[\x1b[39m/home/kali\x1b[34m]\r\n' +
      '└─\x1b[31m#\x1b[39m \x1b[?2004hc\x08\x1b[4mc\x1b[24m\x1b[38;5;244mhmod 777 /var/log/terminal_sessions\x1b[39m\x1b[35D' +
      '\x1b[4md\x1b[24m\x1b[39m' + ' '.repeat(34) + '\x1b[34D\x08\x08\x1b[36mc\x1b[36md\x1b[39m\x1b[38;5;244m wifi_deauth\x1b[39m\x1b[12D \x1b[1mwifi_deauth\x1b[0m\r\n' +
      'Script done on 2026-08-06 03:15:10+00:00 [COMMAND_EXIT_CODE="0"]\r\n';

    expect(isLikelyTerminalSession(raw, '2026-08-06_03-12-58_session.log')).toBe(true);
    const res = parseTerminalSession(raw);
    expect(res.isTerminalLog).toBe(true);
    expect(res.hasAnsiColors).toBe(true);
    expect(res.metadata.term).toBe('xterm-256color');
    expect(res.metadata.tty).toBe('/dev/pts/1');
    expect(res.metadata.columns).toBe(77);
    expect(res.metadata.rows).toBe(28);
    expect(res.metadata.exitCode).toBe('0');
    expect(res.metadata.titles).toEqual(['root@kali-raspberrypi: /home/kali']);

    // Ensure no raw escape chars leaked
    expect(res.plainText).not.toContain('\x1b');
    expect(res.plainText).toContain('┌──(root㉿kali-raspberrypi)-[/home/kali]');
    expect(res.plainText).toContain('Desktop  Music  wlan1mon.pcap');
    // Autosuggestion ghosting was overwritten by CSI 35D spaces + `cd wifi_deauth`
    expect(res.plainText).not.toContain('chmod 777');
    expect(res.commands.map(c => c.command)).toContain('cd wifi_deauth');

    // Verify SGR styles captured for colored Desktop folder
    const lsLine = res.lines.find(l => l.plain.includes('Desktop'));
    expect(lsLine).toBeTruthy();
    const desktopSpan = lsLine!.spans.find(s => s.text.includes('Desktop'));
    expect(desktopSpan?.style.fg).toBe('#539bf5');
    expect(desktopSpan?.style.bold).toBe(true);
  });

  it('supports 256-color, 24-bit RGB, inverse, ED scrollback preservation and alt-screen capture', () => {
    const raw =
      'root@host:/tmp# echo hi\r\n' +
      '\x1b[38;5;208morange256\x1b[0m \x1b[38;2;12;34;56mrgb\x1b[0m \x1b[7minv\x1b[27m\r\n' +
      '\x1b[2J' +
      '\x1b[?1049hALT_CONTENT\r\n\x1b[?1049l' +
      'root@host:/tmp# id\r\n';

    const res = parseTerminalSession(raw);
    expect(res.plainText).toContain('orange256 rgb inv');
    expect(res.plainText).toContain('ALT_CONTENT');
    expect(res.commands.map(c => c.command)).toEqual(['echo hi', 'id']);

    const colorLine = res.lines.find(l => l.plain.includes('orange256'))!;
    expect(colorLine.spans.find(s => s.text === 'orange256')?.style.fg).toMatch(/^#[0-9a-f]{6}$/);
    expect(colorLine.spans.find(s => s.text === 'rgb')?.style.fg).toBe('#0c2238');
    expect(colorLine.spans.find(s => s.text === 'inv')?.style.inverse).toBe(true);
  });
});
