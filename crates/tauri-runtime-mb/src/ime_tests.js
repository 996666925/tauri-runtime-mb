(function () {
  try {
    function check(condition, message) { if (!condition) { throw new Error(message); } }
    var ime = window.__mbIme, el = document.getElementById('a'), events = [];
    function update(text, cursor, result) {
      ime.update({ text: text, cursor: cursor || 0, result: result === undefined ? null : result });
    }
    ['compositionstart', 'compositionupdate', 'compositionend', 'input'].forEach(function (type) {
      el.addEventListener(type, function (e) { events.push([type, e.data, e.isComposing]); });
    });
    el.focus(); el.value = 'left-middle-right'; el.setSelectionRange(5, 11);
    ime.start(); update('ni', 2);
    check(el.value === 'left-ni-right', 'preedit replaces the selection');
    check(el.selectionStart === 7, 'preedit caret');
    update("ni'hao", 2);
    check(el.value === "left-ni'hao-right", 'preedit updates replace previous text');
    check(el.selectionStart === 7, 'IMM cursor position inside preedit');
    var decoration = window.imeTestRoot.lastChild;
    check(decoration.childElementCount > 0, 'inline composition underline');
    check(getComputedStyle(decoration.firstChild).borderBottomStyle === 'dotted', 'underline style');
    check(getComputedStyle(decoration.firstChild).color === getComputedStyle(el).color, 'CSS text color');
    var caret = window.reports[window.reports.length - 1], rect = el.getBoundingClientRect();
    check(caret.x > rect.left + 12 && caret.y > rect.top, 'caret includes padding and preceding text');
    check(caret.height > 15, 'caret follows font size');
    check(events.some(function (e) { return e[0] === 'input' && e[2] === true; }), 'composing input event');
    update(null, 0, '你好'); ime.end();
    check(el.value === 'left-你好-right', 'commit once and remove preedit');
    check(decoration.childElementCount === 0, 'clear underline on commit');
    check(events.filter(function (e) { return e[0] === 'compositionend'; }).length === 1, 'composition ends once');
    check(document.execCommand('undo'), 'commit enters native undo history');
    check(el.value === 'left-middle-right', 'undo restores original selection');

    el.value = 'abc'; el.setSelectionRange(1, 2, 'backward');
    ime.start(); update('qux', 1); ime.end();
    check(el.value === 'abc' && el.selectionStart === 1 && el.selectionEnd === 2,
      'cancel restores selected text');
    check(el.selectionDirection === 'backward', 'cancel restores selection direction');
    update(null, 0, '😀'); ime.end();
    check(el.value === 'a😀c', 'supplementary character commit without preedit');

    el.value = ''; el.setSelectionRange(0, 0);
    ime.start(); update('yi', 2); update('er', 2, '一');
    check(el.value === '一er', 'commit and next composition in one IMM message');
    update(null, 0, '二'); ime.end();
    check(el.value === '一二', 'continuous composition');

    el.value = 'keep'; el.setSelectionRange(4, 4); ime.start(); update('ni', 2);
    var other = document.getElementById('b'); other.focus();
    check(el.value === 'keep', 'focus change clears old preedit');
    update('hao', 3); update(null, 0, '好'); ime.end();
    check(other.value === '好' && el.value === 'keep', 'new editor receives its own composition');

    var multi = document.getElementById('multi'); multi.focus();
    multi.value = 'first\nsecond'; multi.setSelectionRange(8, 8);
    ime.start(); update('ni', 2); update(null, 0, '你'); ime.end();
    check(multi.value === 'first\nse你cond', 'textarea composition');

    el.focus(); el.value = 'x'.repeat(100); el.setSelectionRange(100, 100);
    ime.start(); update('nihao', 5);
    caret = window.reports[window.reports.length - 1]; rect = el.getBoundingClientRect();
    check(caret.x >= rect.left && caret.x <= rect.right, 'long input keeps caret visible');
    ime.end();

    multi.focus(); multi.value = 'line\n'.repeat(40); multi.setSelectionRange(multi.value.length, multi.value.length);
    ime.start(); update('ni', 2);
    caret = window.reports[window.reports.length - 1]; rect = multi.getBoundingClientRect();
    check(caret.y >= rect.top && caret.y + caret.height <= rect.bottom, 'textarea scroll keeps caret visible');
    ime.end();

    var rich = document.getElementById('rich'); rich.innerHTML = 'a<b>middle</b>z'; rich.focus();
    var selection = window.getSelection(), range = document.createRange();
    range.selectNodeContents(rich.querySelector('b')); selection.removeAllRanges(); selection.addRange(range);
    ime.start(); update('ni', 2); update(null, 0, '你好'); ime.end();
    check(rich.textContent === 'a你好z' && rich.querySelector('b').textContent === '你好', 'rich text preserves format');
    range.selectNodeContents(rich.querySelector('b')); selection.removeAllRanges(); selection.addRange(range);
    ime.start(); update('cancel', 3); ime.end();
    check(rich.innerHTML === 'a<b>你好</b>z', 'rich text cancellation restores markup');

    var locked = document.getElementById('locked'); locked.focus();
    ime.start(); update('ni', 2); update(null, 0, '你'); ime.end();
    check(locked.value === 'locked', 'readonly input is not edited');
    check(window.reports[window.reports.length - 1].height === 0, 'readonly input clears caret');
    return 'ok';
  } catch (error) { return String(error.stack || error); }
})();
