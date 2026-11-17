using System.Diagnostics;
using NotBadSearch.Core;
using Microsoft.CommandPalette.Extensions;
using Microsoft.CommandPalette.Extensions.Toolkit;

namespace NotBadSearch.CommandPalette;

internal sealed partial class SearchItem : ListItem
{
    private readonly Details _details;
    private readonly SearchHit _hit;
    private readonly string _query;
    private readonly ISearchBackend _backend;
    private readonly SemaphoreSlim _slots;
    private readonly CancellationToken _token;
    private int _started;
    public SearchItem(SearchHit hit, string query, ISearchBackend backend, SemaphoreSlim slots, CancellationToken token)
        : base(new FileAction(hit.Path, false))
    {
        _hit = hit; _query = query; _backend = backend; _slots = slots; _token = token;
        Title = hit.Name;
        Subtitle = hit.Snippets is { Length: > 0 } ? $"Line {hit.Snippets[0].Line}: {hit.Snippets[0].Text}" : hit.Path;
        Icon = new IconInfo("\uE8A5");
        _details = new Details { Title = hit.Name, Body = PreviewMarkdown.Escape(hit.Path) + "\n\nLoading preview…" };
        MoreCommands = [
            new CommandContextItem(new FileAction(hit.Path, true)),
            new CommandContextItem(new AnonymousCommand(() => ClipboardHelper.SetText(hit.Path)) { Name = "Copy path", Result = CommandResult.KeepOpen() }),
        ];
    }
    public override IDetails? Details
    {
        get { if (Interlocked.Exchange(ref _started, 1) == 0) _ = LoadPreviewAsync(); return _details; }
        set { }
    }
    private async Task LoadPreviewAsync()
    {
        var entered = false;
        try
        {
            await _slots.WaitAsync(_token).ConfigureAwait(false);
            entered = true;
            var preview = await _backend.PreviewAsync(_hit.Path, _query, _token).ConfigureAwait(false);
            _token.ThrowIfCancellationRequested();
            _details.Body = PreviewMarkdown.Render(_hit.Path, preview);
        }
        catch (OperationCanceledException) { }
        catch (Exception ex) { _details.Body = PreviewMarkdown.Escape(_hit.Path) + "\n\n" + PreviewMarkdown.Escape(ex.Message); }
        finally { if (entered) _slots.Release(); }
    }
}

internal sealed partial class FileAction : InvokableCommand
{
    private readonly string _path;
    private readonly bool _reveal;
    public FileAction(string path, bool reveal) { _path = path; _reveal = reveal; Name = reveal ? "Show in File Explorer" : "Open file"; }
    public override ICommandResult Invoke()
    {
        try
        {
            if (_reveal)
            {
                var start = new ProcessStartInfo("explorer.exe") { UseShellExecute = false };
                start.ArgumentList.Add("/select," + _path);
                Process.Start(start);
            }
            else Process.Start(new ProcessStartInfo(_path) { UseShellExecute = true });
            return CommandResult.Dismiss();
        }
        catch (Exception ex) { return CommandResult.ShowToast("Cannot open file: " + ex.Message); }
    }
}
