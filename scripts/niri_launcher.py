"""Migrate the known niri-stack Thunar helper without replacing user scripts."""
import shlex

BEGIN = '# BEGIN quicklook managed file-manager launcher\n'
END = '# END quicklook managed file-manager launcher\n'
COMMAND = 'exec env -u GTK_IM_MODULE GDK_BACKEND=wayland thunar "$@"'


def remove(text):
    if BEGIN not in text and END not in text:
        return text
    if text.count(BEGIN) != 1 or text.count(END) != 1:
        raise ValueError('文件管理器启动脚本的 Quick Look 标记不完整，未改动')
    start, finish = text.index(BEGIN), text.index(END)
    if finish < start:
        raise ValueError('文件管理器启动脚本的 Quick Look 标记顺序错误，未改动')
    return text[:start] + text[finish + len(END):]


def integrate(text, launcher):
    original = remove(text)
    # Do not try to parse arbitrary shell or replace another file manager.
    if original.splitlines().count(COMMAND) != 1:
        return None
    quoted = shlex.quote(str(launcher))
    block = (BEGIN + f'if [ -x {quoted} ]; then\n'
             f'    exec env -u GTK_IM_MODULE GDK_BACKEND=wayland {quoted} "$@"\n'
             'fi\n' + END)
    return original.replace(COMMAND, block + COMMAND, 1)
