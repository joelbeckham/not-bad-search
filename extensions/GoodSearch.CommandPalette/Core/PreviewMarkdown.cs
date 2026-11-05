using System.Text;

namespace GoodSearch.Core;

public static class PreviewMarkdown
{
    public static string Escape(string text)
    {
        var output = new StringBuilder();
        foreach (var c in text)
        {
            if ("\\`*_{}[]<>()!#+-|".Contains(c)) output.Append('\\');
            output.Append(c);
        }
        return output.ToString();
    }
    public static string Render(string path, PreviewResult preview)
    {
        var header = Escape(path) + "\n\n";
        if (preview.Kind == "image" && preview.ImagePath is { } image
            && Path.IsPathFullyQualified(image)
            && Uri.TryCreate(image, UriKind.Absolute, out var uri) && uri.IsFile)
            return header + "![File preview](" + uri.AbsoluteUri.Replace("(", "%28").Replace(")", "%29") + ")";
        if (preview.Kind != "text") return header + Escape(preview.Message ?? "Preview unavailable for this file type.");
        // Literal file text cannot inject markdown images or links.
        var lines = (preview.Text ?? "").Replace("\r\n", "\n").Replace('\r', '\n').Split('\n');
        var body = string.Join("\n", lines.Select((line, i) => $"    {preview.StartLine + i,5}  {line}"));
        return header + body + (preview.Truncated ? "\n\n_Preview truncated._" : "");
    }
}
