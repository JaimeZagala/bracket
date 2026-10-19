// Synthetic C# for the symbol tests: no real code, every construct the extractor must handle.
using System;
using System.Collections.Generic;

namespace Demo.Billing;

/// <summary>Raised when an invoice changes.</summary>
public delegate void InvoiceChanged(object sender, EventArgs e);

[Serializable]
public sealed class Invoice<TLine> : IComparable<Invoice<TLine>> where TLine : class
{
    private readonly List<TLine> _lines = [], _removed = [];
    public static int Count;

    public event InvoiceChanged? Changed;

    public event EventHandler Saved
    {
        add { }
        remove { }
    }

    public Invoice(string number) => Number = number;

    public string Number { get; init; }

    public decimal Total => 0m;

    public TLine this[int index] => _lines[index];

    public int CompareTo(Invoice<TLine>? other) => string.CompareOrdinal(Number, other?.Number);

    public string Describe()
    {
        // Braces and keywords inside strings must not confuse anything.
        var raw = """
            class NotAClass { void NotAMethod() {} }
            """;
        var text = $"{{ {Number} }} class Fake {{ }}";
        return raw + text;
    }

#if DEBUG
    internal void Dump() { }
#endif

    private sealed class Line(string sku, int quantity)
    {
        public string Sku { get; } = sku;
        public int Quantity { get; } = quantity;
    }
}

public record Customer(string Name, string Email);

public record struct Money(decimal Amount, string Currency);

public readonly struct Point
{
    public int X { get; }
    public int Y { get; }
}

public interface IInvoiceStore
{
    Invoice<object>? Find(string number);
    int Count { get; }
}

public enum Status
{
    Draft,
    Sent = 2,
    Paid,
}
