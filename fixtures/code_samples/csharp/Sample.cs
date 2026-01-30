namespace Example;

public class Config
{
    public string Name { get; set; }
    public int Value { get; set; }

    public Config(string name, int value)
    {
        Name = name;
        Value = value;
    }

    public string Display()
    {
        return $"{Name}: {Value}";
    }
}

public interface IProcessor
{
    string Process(string input);
    bool Validate();
}

public enum Status
{
    Active,
    Inactive,
    Pending
}
