pragma Singleton

import QtQuick
import QtQuick.Dialogs
import Quickshell

// The system's file and folder pickers (the desktop's dialog through Qt).
//   Pick.file(startPath, path => …, "Programs (*.exe *.sh)")   Pick.folder(startPath, path => …)
// A cancelled pick calls nothing.
Singleton {
    id: root

    property var _done: null

    function _dir(path) {
        if (!path)
            return Paths.url(Paths.home);
        return Paths.url(path);
    }
    function _local(url) {
        return decodeURIComponent(String(url).replace(/^file:\/\//, ""));
    }

    function file(start, done, filter) {
        _done = done;
        fileDialog.nameFilters = filter ? [filter, "All files (*)"] : ["All files (*)"];
        // A file to start from: open its folder
        fileDialog.currentFolder = _dir(start && /\.[^/]+$/.test(start) ? start.replace(/\/[^/]*$/, "") : start);
        fileDialog.open();
    }
    function folder(start, done) {
        _done = done;
        folderDialog.currentFolder = _dir(start);
        folderDialog.open();
    }

    FileDialog {
        id: fileDialog
        title: "Choose a file"
        onAccepted: if (root._done) root._done(root._local(selectedFile))
    }
    FolderDialog {
        id: folderDialog
        title: "Choose a folder"
        onAccepted: if (root._done) root._done(root._local(selectedFolder))
    }
}
