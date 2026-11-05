using GoodSearch.Core;
using Microsoft.CommandPalette.Extensions;
using Microsoft.CommandPalette.Extensions.Toolkit;

namespace GoodSearch.CommandPalette;

internal sealed partial class SearchPage : DynamicListPage, IDisposable
{
    private readonly ISearchBackend _backend;
    // Serialize publications without holding a lock across WinRT callbacks.
    private readonly TaskFactory _dispatch = new(new ConcurrentExclusiveSchedulerPair(
        TaskScheduler.Default, maxConcurrencyLevel: 1).ExclusiveScheduler);
    private readonly SemaphoreSlim _previewSlots = new(2);
    private CancellationTokenSource? _pending;
    private IListItem[] _items = [];
    private bool _disposed;
    public SearchPage(ISearchBackend backend)
    {
        _backend = backend;
        Id = "good-search.search"; Title = "Good Search"; Name = "Search";
        Icon = new IconInfo("\uE721"); PlaceholderText = "Search indexed files…"; ShowDetails = true;
        EmptyContent = new CommandItem(new NoOpCommand()) { Title = "Type to search", Subtitle = "The Rust IPC backend must be running." };
    }
    public override IListItem[] GetItems() => Volatile.Read(ref _items);
    public override void UpdateSearchText(string oldSearch, string newSearch)
    {
        _ = _dispatch.StartNew(() =>
        {
            if (_disposed) return;
            _pending?.Cancel();
            _pending = new CancellationTokenSource();
            Volatile.Write(ref _items, []);
            IsLoading = !string.IsNullOrWhiteSpace(newSearch);
            EmptyContent = new CommandItem(new NoOpCommand()) { Title = IsLoading ? "Searching…" : "Type to search" };
            RaiseItemsChanged();
            if (IsLoading) _ = SearchAsync(newSearch, _pending);
        });
    }
    private async Task SearchAsync(string query, CancellationTokenSource request)
    {
        try
        {
            if (string.IsNullOrWhiteSpace(query)) return;
            await Task.Delay(120, request.Token).ConfigureAwait(false);
            var result = await _backend.SearchAsync(query, request.Token).ConfigureAwait(false);
            request.Token.ThrowIfCancellationRequested();
            if (result.Hits is null) throw new BackendException("Backend omitted the hits array.");
            var items = result.Hits.Take(100)
                .Where(hit => !string.IsNullOrWhiteSpace(hit.Path) && !string.IsNullOrWhiteSpace(hit.Name) && Path.IsPathFullyQualified(hit.Path))
                .Select(hit => (IListItem)new SearchItem(hit, query, _backend, _previewSlots, request.Token)).ToArray();
            _ = _dispatch.StartNew(() =>
            {
                if (_disposed || _pending != request || request.IsCancellationRequested) return;
                Volatile.Write(ref _items, items);
                EmptyContent = new CommandItem(new NoOpCommand()) { Title = "No matching files", Subtitle = "Try another query." };
                IsLoading = false;
                RaiseItemsChanged();
            });
        }
        catch (OperationCanceledException) { }
        catch (Exception ex)
        {
            _ = _dispatch.StartNew(() =>
            {
                if (_disposed || _pending != request || request.IsCancellationRequested) return;
                Volatile.Write(ref _items, []);
                EmptyContent = new CommandItem(new AnonymousCommand(() => UpdateSearchText(query, query))
                    { Name = "Retry", Result = CommandResult.KeepOpen() })
                    { Title = "Good Search backend unavailable", Subtitle = ex.Message };
                IsLoading = false;
                RaiseItemsChanged();
            });
        }
    }
    public void Dispose() => _ = _dispatch.StartNew(() => { _disposed = true; _pending?.Cancel(); });
}
