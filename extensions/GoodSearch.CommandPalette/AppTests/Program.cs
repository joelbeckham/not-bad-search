using System.Threading.Channels;
using GoodSearch.CommandPalette;
using GoodSearch.Core;

static void Assert(bool value, string message) { if (!value) throw new Exception(message); }
var backend = new ControlledBackend();
using var page = new SearchPage(backend);
var changes = Channel.CreateUnbounded<bool>();
page.ItemsChanged += (sender, args) => { _ = page.GetItems(); changes.Writer.TryWrite(true); };
async Task WaitFor(Func<bool> condition)
{
    using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(5));
    while (!condition()) await changes.Reader.ReadAsync(deadline.Token);
}
page.UpdateSearchText("", "old");
var old = await backend.Calls.Reader.ReadAsync().AsTask().WaitAsync(TimeSpan.FromSeconds(5));
page.UpdateSearchText("old", "new");
var current = await backend.Calls.Reader.ReadAsync().AsTask().WaitAsync(TimeSpan.FromSeconds(5));
Assert(old.Token.IsCancellationRequested, "Previous query canceled");
current.Result.SetResult(new([new(@"C:\new.md", "new.md", [new(7, "matching excerpt")])]));
await WaitFor(() => page.GetItems().Length == 1 && !page.IsLoading);
Assert(page.GetItems()[0].Title == "new.md", "Current result published");
Assert(page.GetItems()[0].Subtitle == "Line 7: matching excerpt", "Snippet displayed");
Assert(backend.PreviewCount == 0, "No eager preview request");
var details = page.GetItems()[0].Details!;
await backend.PreviewCalled.Task.WaitAsync(TimeSpan.FromSeconds(5));
Assert(details == page.GetItems()[0].Details && backend.PreviewCount == 1, "Preview reused per result");
old.Result.SetResult(new([new(@"C:\old.md", "old.md")]));
page.UpdateSearchText("new", "");
await WaitFor(() => page.GetItems().Length == 0 && !page.IsLoading);
Assert(current.Token.IsCancellationRequested, "Preview canceled with query");
Assert(page.GetItems().Length == 0, "Late response cannot restore stale results");
page.UpdateSearchText("", "broken");
var broken = await backend.Calls.Reader.ReadAsync().AsTask().WaitAsync(TimeSpan.FromSeconds(5));
broken.Result.SetException(new BackendException("test backend offline"));
await WaitFor(() => page.EmptyContent?.Title == "Good Search backend unavailable" && !page.IsLoading);
Assert(page.EmptyContent!.Subtitle.Contains("test backend offline"), "Backend error visible");
Console.WriteLine("PASS latest-query wins, cancellation, reentrant notifications, snippets, lazy previews, empty query, backend errors");

sealed record SearchCall(string Query, CancellationToken Token, TaskCompletionSource<SearchResult> Result);
sealed class ControlledBackend : ISearchBackend
{
    public Channel<SearchCall> Calls { get; } = Channel.CreateUnbounded<SearchCall>();
    public TaskCompletionSource PreviewCalled { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    public int PreviewCount;
    public Task<SearchResult> SearchAsync(string query, CancellationToken token)
    {
        var result = new TaskCompletionSource<SearchResult>(TaskCreationOptions.RunContinuationsAsynchronously);
        Calls.Writer.TryWrite(new(query, token, result));
        return result.Task; // Ignore cancellation to exercise stale result protection.
    }
    public async Task<PreviewResult> PreviewAsync(string path, string query, CancellationToken token)
    {
        Interlocked.Increment(ref PreviewCount);
        PreviewCalled.TrySetResult();
        await Task.Delay(Timeout.Infinite, token);
        return new("unavailable");
    }
}
